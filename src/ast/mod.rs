use crate::types::Type;

enum Value {
    Constant(Type),
    Variable(Type),
    Resource,
    Expression(Box<Expression>)
}

enum Operation {
    Addition(Value, Value),
    Subtraction(Value, Value),
    Multiplication(Value, Value),
    Division(Value, Value),
}

enum Expression {
    Value(Value),
    Operation(Operation)
}

struct Context {

}

enum Sequential {
    Integer(i32),
    Char(char),
    CChar(u8)
}

struct Range {
    start: Sequential,
    end: Sequential
}

enum ResourceName {
    Terminal(String),
    //ArrayProduct(String, Array, String),
    RangeProduct(String, Range, String)
}

enum ResourceKind {
    Property, // set/get/subscribe(sugar on stream?), default, allowed, values
    Function, // fn(args) -> value
    Stream,   // value,value,value... subscribe, unsubscribe, backpressure, bandwith limit
    //User      // everything else
}

struct Resource {
    id: u32,
    name: ResourceName,
    children: Vec<Resource>,
    kind: ResourceKind,
    r#type: Type // underlying type
    //meta: // all additional fields goes here
}