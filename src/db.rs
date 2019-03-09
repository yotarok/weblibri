use rusqlite::{Connection, OpenFlags};
use std::path::PathBuf;
use std::str::FromStr;

use rusoto_core::{Region};
use rusoto_s3::{S3,S3Client,GetObjectRequest,GetObjectError};
use std::sync::{Arc, Mutex};
use std::fs::File;
use std::io::{Write,BufWriter};
use futures::stream::Stream;
use futures::Future;
use std::io::Error;
use hyper::Uri;

pub trait DBConnector : Send + Sync {
    fn get_connection(&self) -> Connection;
}

pub struct LocalDBConnector {
    dbpath: PathBuf
}

impl LocalDBConnector {
    pub fn new(dbpath: &String) -> Self {
        LocalDBConnector {
            dbpath: PathBuf::from(dbpath)
        }
    }
}

impl DBConnector for LocalDBConnector {
    fn get_connection(&self) -> Connection {
        Connection::open_with_flags(
            self.dbpath.clone(),
            OpenFlags::SQLITE_OPEN_READ_ONLY
        ).unwrap()
    }
}

pub struct S3DBConnector {
    s3_region: Region,
    s3_bucket: String,
    s3_key: String,
    local_dbpath: PathBuf,
    last_update: Arc<Mutex<Option<String>>>
}

impl S3DBConnector {
    pub fn new(dburi: &String, region: &String) -> Self {
        let region = Region::from_str(region).expect("Unknown region name provided");
        let s3uri: Uri = dburi.parse().unwrap();
        let key = String::from(s3uri.path().trim_start_matches('/'));

        // TODO: local_dbpath is hard-coded.
        S3DBConnector {
            s3_bucket: String::from(s3uri.host().unwrap()),
            s3_key: key,
            s3_region: region,
            local_dbpath: PathBuf::from("/tmp/cached_metadata.db"),
            last_update: Arc::new(Mutex::new(None))
        }
    }
}

fn download_stream<S>(stream: S, dest: &PathBuf) where S: Stream<Item = Vec<u8>, Error = Error> {
    let dest = File::create(dest).expect("Failed to open local cache file");
    let mut dest = BufWriter::new(dest);
    stream.for_each(move |v| {
        dest.write(&v).map(|_| ())
    }).wait().expect("Download failed");
}

impl DBConnector for S3DBConnector {
    fn get_connection(&self) -> Connection {
        let client = S3Client::new(self.s3_region.clone());

        {
            let mut cur_update = self.last_update.lock().unwrap();

            let mut req = GetObjectRequest::default();
            req.bucket = self.s3_bucket.clone();
            req.key = self.s3_key.clone();

            req.if_modified_since = cur_update.clone();


            let res = client.get_object(req).sync();
            match res {
                Err(GetObjectError::NoSuchKey(k)) => {
                    panic!("No such key: {}", k);
                },
                Err(GetObjectError::Credentials(_)) => {
                    panic!("credential");
                },
                Err(GetObjectError::Unknown(_)) => {
                    // TODO: This is really problematic, but since there's no
                    // way to obtain status code now, unknown error is assumed
                    // to be 304.

                    //if e.status.as_u16 == 304 {
                    info!("DB isn't modified since {:?}", cur_update.clone());
                    //} else {
                        //panic!("Other error {:?}", e);
                    //}
                },
                Err(e) => {
                    panic!("Other error {:?}", e);
                },
                Ok(out) => {
                    let body = out.body.unwrap();
                    info!("Downloading metadata...");
                    download_stream(body, &self.local_dbpath);
                    info!("Last modified will be updated to: {:?}", out.last_modified);
                    *cur_update = out.last_modified;
                }
            }
        }
        Connection::open_with_flags(
            self.local_dbpath.clone(),
            OpenFlags::SQLITE_OPEN_READ_ONLY
        ).unwrap()
    }

}


#[derive(Serialize, Deserialize)]
pub struct Book {
    id: i64,
    title: String,
    author_sort: String,
    uuid: String,
    available_data: Vec<String>
}

pub struct BookList<'a> {
    conn: &'a Connection,
}

impl<'a> BookList<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        BookList {
            conn: conn,
        }
    }

    pub fn for_each<F>(&self, f: F) where F: FnMut(&Book) {
        let mut f = f;
        let mut stmt = self.conn.prepare("
SELECT books.id, title, author_sort, uuid, group_concat(data.format)
  FROM books
  INNER JOIN data WHERE data.book = books.id GROUP BY books.id").unwrap();
        let mut rows = stmt.query(&[]).unwrap();
        while let Some(result_row) = rows.next() {
            let row = result_row.unwrap();

            let formats: String = row.get(4);
            let book = Book {
                id: row.get(0),
                title: row.get(1),
                author_sort: row.get(2),
                uuid: row.get(3),
                available_data: formats.split(',').map(
                   |s| s.to_string()).collect()
            };
            f(&book)
        }
    }
}
