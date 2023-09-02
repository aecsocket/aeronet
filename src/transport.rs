pub trait Message: 'static + Send + Sync + Clone {}

pub trait TransportSettings: 'static + Send + Sync {
    type C2S: Message;
    type S2C: Message;
}
