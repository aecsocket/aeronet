use aeronet::Message;

#[derive(Message)]
struct MyStructMsg;

#[derive(Message)]
enum MyEnumMsg {}

fn assert_message<T: Message>() {}

#[test]
fn test() {
    assert_message::<MyStructMsg>();
    assert_message::<MyEnumMsg>();
}
