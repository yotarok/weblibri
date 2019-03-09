use std::{io, fmt};
use std::path::PathBuf;
use std::error::Error;
use std::sync::mpsc::Receiver;
use std::process::{Command, ExitStatus};

use cache::check_cache_availability;

#[derive(Debug)]
enum ConversionError {
    EpubConversionCommandError(io::Error),
    EpubConversionError(ExitStatus),
    EpubDeflationCommandError(io::Error),
    EpubDeflationError(ExitStatus),
    CleanUpError(io::Error)
}
use self::ConversionError::{EpubConversionCommandError,EpubConversionError,
                            EpubDeflationCommandError,EpubDeflationError,
                            CleanUpError};

impl Error for ConversionError {
    fn description(&self) -> &str {
        "conversion error"
    }

    fn cause(&self) -> Option<&Error> {
        match self {
            EpubConversionCommandError(e) => Some(e),
            EpubDeflationCommandError(e) => Some(e),
            CleanUpError(e) => Some(e),
            _ => None
        }
    }
}

impl fmt::Display for ConversionError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            EpubConversionCommandError(e) =>
                write!(f, "Failed to launch converter: {}", e),
            EpubConversionError(code) =>
                write!(f, "Converter exited with an error code: {:?}", code),
            EpubDeflationCommandError(e) =>
                write!(f, "Failed to launch unzip: {}", e),
            EpubDeflationError(code) =>
                write!(f, "Unzipper exited with an error code: {:?}", code),
            CleanUpError(e) =>
                write!(f, "Failed to remove temporary epub file: {}", e),
        }
    }
}

fn convert_to_epub(converter_bin: &str,
                   srcpath: &PathBuf, dstpath: &PathBuf)
                   -> Result<(), ConversionError>  {
    info!("Convert {:?} to epub and extract to {:?}...", srcpath, dstpath);
    let result = Command::new(converter_bin)
        .arg(srcpath.to_str().unwrap())
        .arg(dstpath.to_str().unwrap())
        .arg("--no-default-epub-cover")
        .arg("--output-profile")
        .arg("tablet")
    //.arg("--extract-to")
    //.arg(dstpath)
        //.arg("--title")
        //.arg(title)
        .status();

    match result {
        Ok(code) if code.success() => Ok(()),
        Ok(code) => Err(EpubConversionError(code)),
        Err(e) => Err(EpubConversionCommandError(e))
    }
}

fn convert(converter_bin: &str, src: &str, dest: &str)
           -> Result<(), ConversionError> {
    let src: PathBuf = PathBuf::from(src);
    let dest: PathBuf = PathBuf::from(dest);

    if check_cache_availability(&dest) {
        return Ok(())
    }

    let (epubpath, need_cleanup) = if src.to_str().unwrap().ends_with(".epub") {
        (src.clone(), false)
    } else {
        let mut epubpath = dest.clone();
        epubpath.set_extension("epub");
        try!(convert_to_epub(converter_bin, &src, &epubpath));
        (epubpath, true)
    };

    let result = Command::new("unzip")
        .arg("-d").arg(dest)
        .arg(&epubpath)
        .status();
    try!(match result {
        Ok(code) if code.success() => Ok(()),
        Ok(code) => Err(EpubDeflationError(code)),
        Err(e) => Err(EpubDeflationCommandError(e))
    });

    if need_cleanup {
        let result = Command::new("rm")
            .arg(&epubpath)
            .status();
        try!(match result {
            Ok(_) => Ok(()),
            Err(e) => Err(CleanUpError(e))
        });
    };

    Ok(())
}

pub fn worker_loop(converter_bin: &str,
               task_rx: Receiver<(PathBuf, PathBuf)>) {
    loop {
        let (src, dest) = task_rx.recv().unwrap();

        let result = convert(converter_bin,
                             src.to_str().unwrap(),
                             dest.to_str().unwrap());
        match result {
            Ok(_) => {},
            Err(e) => {
                warn!("Convertion failed: {}", e)
            }
        }

    }
}
