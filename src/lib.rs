mod types;

include!(concat!(env!("OUT_DIR"), "/hello.rs"));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        assert_eq!(fr_BE::LC_TIME::d_fmt(), "%d/%m/%y");
        assert_eq!(fr_BE::LC_TIME::first_weekday(), 2_i64);
    }
}
