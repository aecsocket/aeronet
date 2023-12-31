use derivative::Derivative;

#[derive(Derivative)]
#[derivative(Debug, Default)]
pub struct ConnectedClient<P> {

}

#[derive(Derivative)]
#[derivative(Debug(bound = ""), Default(bound = ""))]
pub enum ChannelClient<P> {
    #[derivative(Default)]
    Disconnected,
    Connected(ConnectedClient<P>),
}
