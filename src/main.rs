#![feature(box_syntax)]

extern crate actix_web;
extern crate rusqlite;
extern crate serde;
extern crate serde_json;

#[macro_use]
extern crate serde_derive;

#[macro_use]
extern crate askama;

#[macro_use]
extern crate log;

extern crate stderrlog;

extern crate structopt;

extern crate rusoto_core;
extern crate rusoto_s3;
extern crate futures;
extern crate hyper;

use std::path::PathBuf;
use std::sync::{mpsc, Arc};
use std::sync::mpsc::{SyncSender, Receiver};
use std::thread;

use actix_web::{server, App, fs, middleware};
use structopt::StructOpt;

mod db;
mod worker;
mod httphandler;
mod cache;

use worker::worker_loop;
use httphandler::{get_main_page, get_reader_page, get_book_list,
                  get_book_data, get_reader_status,
                  AppConfig};


#[derive(StructOpt, Debug, Clone)]
#[structopt(name = "basic")]
struct Opt {
    #[structopt(short = "d", long = "db")]
    meta_data_db: String,
    #[structopt(short = "s", long = "static-pages", parse(from_os_str))]
    static_path: PathBuf,
    #[structopt(short = "c", long = "cache-dir", parse(from_os_str))]
    cache_path: PathBuf,
    #[structopt(short = "D", long = "data-root-dir", parse(from_os_str))]
    data_path: Option<PathBuf>,
    #[structopt(short = "C", long = "converter", default_value = "ebook-convert")]
    converter_bin: String,
    #[structopt(short = "v", long = "verbose", parse(from_occurrences))]
    verbosity: usize,
    #[structopt(short = "p", long = "app-uri-prefix", default_value = "")]
    app_prefix: String,
    #[structopt(short = "b", long = "bind-to", default_value = "0.0.0.0:8000")]
    bind_to: String,
    #[structopt(long = "s3-region", default_value = "us-east-1")]
    s3_region: String
}

impl Opt {
    pub fn make_app_config(self,
                           conv_task_tx: SyncSender<(PathBuf, PathBuf)>)
                           -> AppConfig {

        // As an intermediate solution, only metadata can be read from S3 directly.
        // In case the meta_data_db is S3 URI, there's no way to resolve data path.
        // Therefore, the startup process will exit with an error code, then.

        if self.meta_data_db.starts_with("s3://") {
            let data_path = self.data_path.expect(
                "Need to specify data path explicitly when metadata is from S3");
            AppConfig {
                db_connector: box db::S3DBConnector::new(
                    &self.meta_data_db,
                    &self.s3_region),
                static_path: self.static_path,
                cache_path: self.cache_path,
                data_path: data_path,
                app_prefix: self.app_prefix,
                conv_task_tx: conv_task_tx,
            }
        } else {
            let default_data_path = {
                let mut p = PathBuf::from(self.meta_data_db.clone());
                p.pop();
                p
            };

            AppConfig {
                db_connector: box db::LocalDBConnector::new(&self.meta_data_db),
                static_path: self.static_path,
                cache_path: self.cache_path,
                data_path: self.data_path.unwrap_or(default_data_path),
                app_prefix: self.app_prefix,
                conv_task_tx: conv_task_tx
            }
        }
    }
}

fn main() {
    let opt = Opt::from_args();

    stderrlog::new().verbosity(opt.verbosity).init().unwrap();

    info!("Starting e-book converter thread...");
    let (tx, rx): (SyncSender<(PathBuf, PathBuf)>, Receiver<(PathBuf, PathBuf)>) =
        mpsc::sync_channel(100);

    let converter_bin = opt.converter_bin.clone();
    thread::spawn(move || {
        worker_loop(&converter_bin, rx);
    });

    let conf = Arc::new(opt.clone().make_app_config(tx));

    server::new(move || {
        App::with_state(conf.clone())
            .prefix(conf.app_prefix.clone())
            .middleware(middleware::Logger::default())
            .resource("/api/booklist.js", |r| r.f(get_book_list))
            .resource("/api/{bookid}/reader_status.js",
                      |r| r.f(get_reader_status))
            .resource("", |r| r.f(get_main_page))
            .resource("/", |r| r.f(get_main_page))
            .resource(
                "/data/{bookid}/{datatype}",
                |r| r.f(get_book_data))
            .resource(
                "/reader/{bookid}",
                |r| r.f(get_reader_page))
            .handler(
                "/book",
                fs::StaticFiles::new(conf.cache_path.to_str().unwrap())
                    .unwrap()
                    .show_files_listing())
            .handler(
                "/",
                fs::StaticFiles::new(conf.static_path.to_str().unwrap())
                    .unwrap()
                    .show_files_listing())
    })
        .bind(&opt.bind_to)
        .expect(&format!("Can not bind to {}", &opt.bind_to))
        .run();
}
