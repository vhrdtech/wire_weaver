use crate::types::{Lit, RangeLit, StrLit, Ty};

#[derive(Debug)]
pub enum Value {
    Constant(Ty),
    Variable(Ty),
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
pub struct Range {
    pub start: RangeLit,
    pub end: RangeLit,
}

#[derive(Debug)]
pub enum ResourceName {
    Terminal(StrLit),
    //ArrayProduct(String, Array, String),
    RangeProduct(StrLit, Range, StrLit),
}

#[derive(Debug)]
pub enum ResourceKind {
    // set/get/subscribe(sugar on stream?), default, allowed, values
    // underlying type is required, maybe not known right away, but can be derived from bits
    Property(Option<Ty>),
    Function, // fn(args) -> value
    Stream(Ty),   // value,value,value... subscribe, unsubscribe, backpressure, bandwith limit
              //User      // everything else
}

#[derive(Debug)]
pub struct Resource {
    pub id: Option<u32>, // required, maybe not provided at all, it that case it can be auto assigned
    pub name: ResourceName, // required and available right away
    pub children: Vec<Resource>, // optional, can be 0 or more children
    pub kind: Option<ResourceKind>, // required, but maybe not known right away
                  //meta: // all additional fields goes here, hash Ident->Expression?
}
