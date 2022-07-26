struct PostcardFlavour<'a> {
    buffer: &'a mut Vec<u8>,
}

impl<'a> postcard::ser_flavors::Flavor for PostcardFlavour<'a> {
    type Output = ();

    fn try_extend(&mut self, data: &[u8]) -> postcard::Result<()> {
        self.buffer.extend_from_slice(data);
        Ok(())
    }

    fn try_push(&mut self, data: u8) -> postcard::Result<()> {
        self.buffer.push(data);
        Ok(())
    }

    fn finalize(self) -> postcard::Result<Self::Output> {
        Ok(())
    }
}

impl<'a> std::ops::Index<usize> for PostcardFlavour<'a> {
    type Output = u8;

    fn index(&self, index: usize) -> &Self::Output {
        &self.buffer[index]
    }
}

impl<'a> std::ops::IndexMut<usize> for PostcardFlavour<'a> {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.buffer[index]
    }
}

pub fn encode_value<'a, T: serde::Serialize>(
    value: &T,
    buffer: &'a mut Vec<u8>,
) -> postcard::Result<&'a [u8]> {
    postcard::serialize_with_flavor(
        value,
        postcard::ser_flavors::Cobs::try_new(PostcardFlavour { buffer })?,
    )?;

    Ok(buffer)
}

pub fn decode_value<'de, T: serde::Deserialize<'de>>(buffer: &'de mut [u8]) -> postcard::Result<T> {
    postcard::from_bytes_cobs(buffer)
}

#[cfg(feature = "async")]
pub fn decode_from_stream<
    S: tokio::io::AsyncBufRead + Unpin,
    T: serde::de::DeserializeOwned,
    E: From<std::io::Error> + From<postcard::Error>,
>(
    stream: S,
) -> impl futures_util::Stream<Item = Result<T, E>> {
    use tokio::io::AsyncBufReadExt;

    futures_util::stream::try_unfold(
        (stream, Vec::new()),
        |(mut stream, mut buffer)| async move {
            buffer.clear();
            let read_size = stream.read_until(0, &mut buffer).await?;

            if read_size == 0 {
                return Ok(None);
            }

            let message = decode_value(&mut buffer)?;

            Ok(Some((message, (stream, buffer))))
        },
    )
}

#[cfg(feature = "async")]
pub fn encode_to_stream<
    S: tokio::io::AsyncWrite + Unpin,
    T: serde::Serialize,
    E: From<std::io::Error> + From<postcard::Error>,
>(
    stream: S,
) -> impl futures_util::Sink<T, Error = E> {
    use tokio::io::AsyncWriteExt;

    futures_util::sink::unfold(
        (stream, Vec::new()),
        |(mut stream, mut buffer), message: T| async move {
            buffer.clear();
            stream
                .write_all(encode_value(&message, &mut buffer)?)
                .await?;

            Ok((stream, buffer))
        },
    )
}
