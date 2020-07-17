#[test]
fn locale_match() {
    let locale = "fr_BE";
    let result = pure_rust_locales::locale_match!(locale => LC_TIME::D_FMT);
    assert_eq!(result.unwrap(), "%d/%m/%y");
}
