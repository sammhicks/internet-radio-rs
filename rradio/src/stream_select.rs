use std::{
    pin::Pin,
    task::{Context, Poll},
};

use futures_util::Stream;

pub struct StreamSelect<T>(pub T);

macro_rules! impl_stream_select {
    ($($($name:ident)*;)*) => {
        $(
            impl<S: Stream $(, $name: Stream<Item = S::Item>)*> Stream for StreamSelect<(S, $($name,)*)> {
                type Item = S::Item;

                fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
                    #[allow(non_snake_case)]
                    let StreamSelect((S, $($name,)*)) = unsafe { self.get_unchecked_mut() };

                    if let Poll::Ready(value) = unsafe { Pin::new_unchecked(S) }.poll_next(cx) {
                        return Poll::Ready(value);
                    }

                    $(
                        if let Poll::Ready(value) = unsafe { Pin::new_unchecked($name) }.poll_next(cx) {
                            return Poll::Ready(value);
                        }
                    )*

                    Poll::Pending
                }
            }
        )*
    };
}

impl_stream_select!(
    ;
    S1;
    S1 S2;
    S1 S2 S3;
);
