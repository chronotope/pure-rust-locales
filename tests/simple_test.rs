#[test]
fn it_works() {
    assert_eq!(pure_rust_locales::fr_BE::LC_TIME::D_FMT, "%d/%m/%y");
    assert_eq!(pure_rust_locales::fr_BE::LC_TIME::FIRST_WEEKDAY, 2_i64);
}
