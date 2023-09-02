pub trait Message: 'static + Send + Sync + Clone {}

impl Message for () {}

pub trait TransportSettings: 'static + Send + Sync {
    type C2S: Message;
    type S2C: Message;
}
