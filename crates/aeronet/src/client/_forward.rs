#[macro_export]
macro_rules! combine_clients {
    (
        $(#[$outer:meta])*
        $vis:vis enum $ty:ident {
            $($variants:ident),+;

            $(#[$connecting_outer:meta])*
            type Connecting = $connecting_vis:vis $connecting_ty:ident;

            $(#[$connected_outer:meta])*
            type Connected = $connected_vis:vis $connected_ty:ident;

            $(#[$message_key_outer:meta])*
            type MessageKey = $message_key_vis:vis $message_key_ty:ident;

            $(#[$poll_error_outer:meta])*
            type PollError = $poll_error_vis:vis $poll_error_ty:ident;

            $(#[$send_error_outer:meta])*
            type SendError = $send_error_vis:vis $send_error_ty:ident;
        }

        $($rest:tt)*
    ) => {
        $(#[$outer])*
        $vis enum $ty {
        }

        impl $crate::client::ClientTransport for $ty {
            type Connecting<'this> = $connecting_ty;

            type Connected<'this> = $connected_ty;

            type MessageKey = $message_key_ty;

            type PollError = $poll_error_ty;

            type SendError = $send_error_ty;

            fn state(&self) -> $crate::client::ClientState<Self::Connecting<'_>, Self::Connected<'_>> {
                match self {
                }
            }

            fn send(
                &mut self,
                msg: impl Into<$crate::bytes::Bytes>,
                lane: impl Into<$crate::lane::LaneIndex>,
            ) -> Result<Self::MessageKey, Self::SendError> {
                match self {}
            }
        }

        $(#[$connecting_outer])*
        $connecting_vis enum $connecting_ty {

        }

        $(#[$connected_outer])*
        $connected_vis enum $connected_ty {

        }

        $(#[$message_key_outer])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        $message_key_vis enum $message_key_ty {

        }

        $(#[$poll_error_outer])*
        $poll_error_vis enum $poll_error_ty {

        }

        $(#[$send_error_outer])*
        $send_error_vis enum $send_error_ty {

        }

        $crate::combine_clients!($($rest)*);
    };
    () => {}
}

#[cfg(test)]
mod tests {
    use crate::client::ClientTransport;

    use super::*;

    fn make<Foo: ClientTransport, Bar: ClientTransport>() {
        combine_clients! {
            /// My combined Foo and Bar client
            pub enum FooBarClient {
                Foo,
                Bar;

                /// Connecting type for [`FooBarClient`].
                type Connecting = MyConnecting;

                /// Connected type for [`FooBarClient`].
                type Connected = MyConnected;

                /// Message key type for [`FooBarClient`].
                type MessageKey = MyMessageKey;

                /// Poll error type for [`FooBarClient`].
                type PollError = MyPollError;

                /// Send error type for [`FooBarClient`].
                type SendError = MySendError;
            }
        }

        // impl MyMessageKey {}
    }
}
