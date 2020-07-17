use std::convert::TryInto;

#[test]
fn locale_match() {
    let locale = "fr_BE".try_into().unwrap();
    let result = pure_rust_locales::locale_match!(locale => LC_TIME::D_FMT);
    assert_eq!(result, "%d/%m/%y");
}
