use aeronet::Message;

#[derive(Debug, Message)]
struct Value {}

#[test]
fn type_is_message() {
    assert_message::<Value>();
}

fn assert_message<T>() where T: Message {}
