use super::*;
use failure::Error;
use futures::stream::Stream;
use futures::prelude::*;
use futures::task::Context;

impl Stream for Watcher {
    type Item = WatchResponse;
    type Error = Error;

    fn poll_next(&mut self, _cx: &mut Context) -> Result<Async<Option<Self::Item>>, Self::Error> {

        if self.reg_key.is_none() {
            return Ok(Async::Ready(None));
        }

        if self.handle.is_none() {
            let (sender, receiver) = channel();
            self.stream_receiver = Some(receiver);
            self.watch_async(sender)?;
        }

        if let Some(ref rx) = self.stream_receiver {
            return match rx.try_recv() {
                Ok(v) => Ok(Async::Ready(Some(v))),
                Err(TryRecvError::Empty) => Ok(Async::Pending),
                Err(e) => Err(format_err!("stream_receiver try_recv: {}", e)),
            };
        }

        Ok(Async::Pending)
    }
}
