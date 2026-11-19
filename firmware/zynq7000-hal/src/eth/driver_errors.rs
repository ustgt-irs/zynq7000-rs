//! Errors of the smoltcp and embassy-net driver.
//!
//! The driver reports its errors to an [ErrorHandler] provided by the application.
use super::tx_descr::TxError;

/// Error of the smoltcp or embassy-net driver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum DriverError {
    /// Transmitting a frame failed.
    Tx(TxError),
    /// A received frame spans multiple descriptor slots. The driver can not handle these, so the
    /// frame was dropped.
    MultiSlotFrame,
    /// Broken frame fragments were received and discarded.
    BrokenFragments,
}

/// Receives the errors of the driver.
///
/// A full TX descriptor list is not reported. It is normal backpressure and the network stack
/// retries later. A stalled TX path shows up as a transmitted frame count which stops growing,
/// see [super::Ethernet::read_tx_statistics].
pub trait ErrorHandler {
    /// Called for every error. It runs within the poll of the network stack, so it should return
    /// quickly and must not block.
    fn on_error(&mut self, error: DriverError);
}

/// Ignores all errors.
impl ErrorHandler for () {
    #[inline]
    fn on_error(&mut self, _error: DriverError) {}
}

impl<ErrHandler: ErrorHandler + ?Sized> ErrorHandler for &mut ErrHandler {
    #[inline]
    fn on_error(&mut self, error: DriverError) {
        (**self).on_error(error)
    }
}
