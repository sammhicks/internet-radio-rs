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
