use aeronet::message::Message;

#[derive(Message)]
struct MyStructMsg;

#[derive(Message)]
enum MyEnumMsg {}

#[test]
fn assert_types() {
    fn assert_message<T: Message>() {}

    assert_message::<MyStructMsg>();
    assert_message::<MyEnumMsg>();
}
