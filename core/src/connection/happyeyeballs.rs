use std::mem;
use std::pin::Pin;
use std::time::Duration;
use std::future::Future;
use std::task::{Poll, Context};
use crate::Multiaddr;
use crate::transport::{Transport, TransportError};
use futures::stream::{Stream, FuturesUnordered};
use futures_timer::Delay;
use pin_project::pin_project;


const STEP: Duration = Duration::from_millis(250);

#[pin_project]
pub struct ConnectFuture<TTrans>
where
    TTrans: Transport + Clone
{
    #[pin]
    transport: TTrans,
    #[pin]
    timer: Delay,
    #[pin]
    queue: FuturesUnordered<TTrans::Dial>,
    want: bool,
    addresses: Vec<Multiaddr>
}

pub fn connect_to<TTrans>(transport: TTrans, addr: Multiaddr, remaining: Vec<Multiaddr>)
    -> ConnectFuture<TTrans>
where
    TTrans: Transport + Clone
{
    let mut addresses = remaining;
    addresses.reverse();
    addresses.push(addr);

    ConnectFuture {
        transport, addresses,
        timer: Delay::new(STEP),
        queue: FuturesUnordered::new(),
        want: false
    }
}

impl<TTrans> Future for ConnectFuture<TTrans>
where
    TTrans: Transport + Clone
{
    type Output = Result<TTrans::Output, TransportError<TTrans::Error>>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();

        *this.want |= this.timer.as_mut().poll(cx).is_ready();

        if mem::replace(this.want, false) {
            if let Some(addr) = this.addresses.pop() {
                let fut = this.transport.clone().dial(addr)?;
                this.queue.push(fut);

                this.timer.as_mut().reset(STEP);
                let _ = this.timer.as_mut().poll(cx);
            }
        }

        match this.queue.as_mut().poll_next(cx) {
            Poll::Ready(Some(Ok(output))) => Poll::Ready(Ok(output)),
            Poll::Ready(Some(Err(err))) if this.queue.is_empty() && this.addresses.is_empty() =>
                Poll::Ready(Err(TransportError::Other(err))),
            Poll::Ready(None) if this.queue.is_empty() && this.addresses.is_empty() =>
                unreachable!("addresses is empty"),
            Poll::Ready(Some(Err(_))) | Poll::Ready(None) => {
                *this.want = true;
                cx.waker().wake_by_ref();
                Poll::Pending
            },
            Poll::Pending => Poll::Pending
        }

    }
}
