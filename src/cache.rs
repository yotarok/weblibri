use std::path::PathBuf;

const READER_CHECKER_FILE: &str = "META-INF/container.xml";

pub fn check_cache_availability(reader_path: &PathBuf) -> bool {
    let mut checker_path = reader_path.clone();
    checker_path.push(READER_CHECKER_FILE);

    checker_path.is_file()
}


