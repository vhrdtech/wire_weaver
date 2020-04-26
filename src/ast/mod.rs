use crate::types::Type;

#[derive(Debug)]
pub enum Value {
    Constant(Type),
    Variable(Type),
    Resource,
    Expression(Box<Expression>),
}

#[derive(Debug)]
pub enum Operation {
    Addition(Value, Value),
    Subtraction(Value, Value),
    Multiplication(Value, Value),
    Division(Value, Value),
}

#[derive(Debug)]
pub enum Expression {
    Value(Value),
    Operation(Operation),
}

#[derive(Debug)]
pub struct Context {}

#[derive(Debug)]
pub enum Sequential {
    Integer(i32),
    Char(char),
    CChar(u8),
}

#[derive(Debug)]
pub struct Range {
    pub start: Sequential,
    pub end: Sequential,
}

#[derive(Debug)]
pub enum ResourceName {
    Terminal(String),
    //ArrayProduct(String, Array, String),
    RangeProduct(String, Range, String),
}

#[derive(Debug)]
pub enum ResourceKind {
    Property, // set/get/subscribe(sugar on stream?), default, allowed, values
    Function, // fn(args) -> value
    Stream,   // value,value,value... subscribe, unsubscribe, backpressure, bandwith limit
              //User      // everything else
}

#[derive(Debug)]
pub struct Resource {
    pub id: Option<u32>,
    pub name: ResourceName,
    pub children: Vec<Resource>,
    pub kind: ResourceKind,
    pub r#type: Type, // underlying type
                  //meta: // all additional fields goes here
}
