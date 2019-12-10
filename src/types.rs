#[allow(dead_code)]
#[derive(Debug)]
pub enum Value {
    String(&'static str),
    Integer(i64),
}
