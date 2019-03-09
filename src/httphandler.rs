use std::path::PathBuf;
use std::sync::mpsc::SyncSender;
use std::sync::Arc;

use actix_web::{HttpRequest, Responder, fs, HttpResponse,
                Either as EitherResponder};
use actix_web::http::StatusCode;
use actix_web::http::header::{ContentDisposition, DispositionType,
                              DispositionParam, Charset,
                              ContentEncoding};
use askama::Template;
use serde_json;
use rusqlite::{Connection};

use db::{BookList,DBConnector};
use cache::check_cache_availability;

pub struct AppConfig {
    pub db_connector: Box<DBConnector>,
    pub static_path: PathBuf,
    pub cache_path: PathBuf,
    pub data_path: PathBuf,
    pub app_prefix: String,
    pub conv_task_tx: SyncSender<(PathBuf, PathBuf)>
}

impl AppConfig {
    pub fn get_meta_data_conn(&self) -> Connection {
        self.db_connector.get_connection()
    }
}

pub type AppState = Arc<AppConfig>;

/// List of all supported formats in the preference order
const PREFERRED_FORMAT: &[&'static str] = &["EPUB", "HTMLZ", "AZW3", "AZW4", "MOBI", "PDF"];

#[derive(Template)]
#[template(path = "main_page.html", escape = "none")]
struct MainPage<'a> {
    app_prefix: &'a str,
}

#[derive(Template)]
#[template(path = "reader_page.html", escape = "none")]
struct ReaderPage<'a> {
    app_prefix: &'a str,
    bookid: i64,
}

pub fn get_main_page(req: &HttpRequest<AppState>) -> HttpResponse {
    HttpResponse::Ok()
        .content_type("text/html")
        .body(MainPage {
            app_prefix: &req.state().app_prefix
        }.render().unwrap())
}


pub fn get_reader_page(req: &HttpRequest<AppState>) -> HttpResponse {
    let bookid: i64 =
        req.match_info().get("bookid").unwrap().parse().unwrap();
    HttpResponse::Ok()
        .content_type("text/html")
        .body(ReaderPage {
            app_prefix: &req.state().app_prefix,
            bookid: bookid,
        }.render().unwrap())
}

pub fn get_book_list(req: &HttpRequest<AppState>) -> impl Responder {
    let conn = req.state().get_meta_data_conn();
    let booklist = BookList::new(&conn);

    let mut list = String::new();
    list.push_str("[");
    let mut is_first = true;
    booklist.for_each(|ref book| {
        if is_first {
            is_first = false;
        } else {
            list.push_str(",");
        }
        list.push_str(&serde_json::to_string(&book).unwrap());
    });
    list.push_str("]");
    list
}

pub fn get_book_data(req: &HttpRequest<AppState>) -> impl Responder {
    let bookid: i64 =
        req.match_info().get("bookid").unwrap().parse().unwrap();
    let datatype = req.match_info().get("datatype").unwrap();

    let conn = req.state().get_meta_data_conn();

    let mut stmt = conn.prepare("
SELECT books.title, books.path, data.name FROM books
 INNER JOIN data WHERE data.book = books.id
   AND books.id = (:bookid)
   AND data.format = (:datatype)").unwrap();

    let mut rows = stmt.query_named(&[
        (":bookid", &bookid),
        (":datatype", &datatype)
    ]).unwrap();
    let resp = match rows.next() {
        Some(row) => {
            let row = row.unwrap();
            let dirname: String = row.get(1);
            let dirname: PathBuf = PathBuf::from(dirname);
            let mut filename: String = row.get(2);
            filename.push('.');
            filename.push_str(&datatype.to_lowercase());

            let mut fullpath = req.state().data_path.clone();
            fullpath.push(dirname);
            fullpath.push(filename);

            let mut download_filename: String = row.get(0);
            download_filename.push('.');
            download_filename.push_str(&datatype.to_lowercase());

            debug!("Serve {:?}", fullpath);
            let mut file = fs::NamedFile::open(fullpath).unwrap();
            file = file.set_content_disposition(ContentDisposition {
                disposition: DispositionType::Inline,
                parameters: vec![
                    DispositionParam::Filename(
                        Charset::Ext("UTF-8".to_string()), None,
                        download_filename.into_bytes())
                ]
            });
            file = file.set_content_encoding(ContentEncoding::Br);

            EitherResponder::A(file)
        }
        None => {
            EitherResponder::B(
                HttpResponse::new(StatusCode::NOT_FOUND)
            )
        }
    };
    resp
}

pub fn get_reader_status(req: &HttpRequest<AppState>) -> impl Responder {
    let do_enqueue =
        req.query().get("enqueue").and_then(|s| s.parse().ok()).unwrap_or(1)
        != 0;
    let conn = req.state().get_meta_data_conn();
    let bookid: i64 =
        req.match_info().get("bookid").unwrap().parse().unwrap();

    // Currently, query reader status automatically enqueues conversion job
    // but this might be not the cleanest solution.

    let mut reader_path = req.state().cache_path.clone();
    reader_path.push(format!("{}", bookid));

    let mut reader_uri = req.state().app_prefix.clone();
    reader_uri.push_str("/reader/");
    reader_uri.push_str(&format!("{}", bookid));

    let mut is_ready = "true";
    if ! check_cache_availability(&reader_path) {
        if do_enqueue {
            let mut stmt = conn.prepare("
SELECT books.path, data.name, data.format
FROM books INNER JOIN data
WHERE data.book = books.id AND books.id = (:bookid)").unwrap();

            let mut rows = stmt.query_named(&[(":bookid", &bookid)]).unwrap();

            // Really need refactoring
            let mut dirname: String = String::new();
            let mut filename: String = String::new();
            let mut min_cost = PREFERRED_FORMAT.len() + 1;
            let mut min_format = String::new();

            while let Some(result_row) = rows.next() {
                let row = result_row.unwrap();
                let format: String = row.get(2);
                let cost =
                    PREFERRED_FORMAT.iter().position(|x| *x == format)
                    .unwrap_or(PREFERRED_FORMAT.len());
                if cost < min_cost {
                    min_cost = cost;
                    min_format = format;
                    dirname = row.get(0);
                    filename = row.get(1);
                }
            }

            filename.push('.');
            filename.push_str(&min_format.to_lowercase());

            let mut src_path = req.state().data_path.clone();
            src_path.push(dirname);
            src_path.push(filename);

            match req.state().conv_task_tx.send((src_path, reader_path)) {
                Ok(_) => {
                    debug!("Status checked, and enqueued the task");
                },
                Err(_) => {
                    warn!("Enqueuing failed")
                }
            }
        } else {
            debug!("Status checked, but didn't enqueue the task");
        }

        is_ready = "false";
    }
    format!(
        r#"{{"is_ready": {}, "uri": "{}"}}"#,
        is_ready, reader_uri)
}
