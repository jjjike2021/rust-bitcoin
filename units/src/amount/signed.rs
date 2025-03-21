// SPDX-License-Identifier: CC0-1.0

//! A signed bitcoin amount.

#[cfg(feature = "alloc")]
use alloc::string::{String, ToString};
use core::str::FromStr;
use core::{default, fmt};

#[cfg(feature = "arbitrary")]
use arbitrary::{Arbitrary, Unstructured};

use super::error::{ParseAmountErrorInner, ParseErrorInner};
use super::{
    parse_signed_to_satoshi, split_amount_and_denomination, Amount, Denomination, Display,
    DisplayStyle, OutOfRangeError, ParseAmountError, ParseError,
};

mod encapsulate {
    /// A signed amount.
    ///
    /// The [`SignedAmount`] type can be used to express Bitcoin amounts that support arithmetic and
    /// conversion to various denominations. The [`SignedAmount`] type does not implement [`serde`]
    /// traits but we do provide modules for serializing as satoshis or bitcoin.
    ///
    /// Warning!
    ///
    /// This type implements several arithmetic operations from [`core::ops`].
    /// To prevent errors due to an overflow when using these operations,
    /// it is advised to instead use the checked arithmetic methods whose names
    /// start with `checked_`. The operations from [`core::ops`] that [`SignedAmount`]
    /// implements will panic when an overflow occurs.
    ///
    /// # Examples
    ///
    /// ```
    /// # #[cfg(feature = "serde")] {
    /// use serde::{Serialize, Deserialize};
    /// use bitcoin_units::SignedAmount;
    ///
    /// #[derive(Serialize, Deserialize)]
    /// struct Foo {
    ///     // If you are using `rust-bitcoin` then `bitcoin::amount::serde::as_sat` also works.
    ///     #[serde(with = "bitcoin_units::amount::serde::as_sat")]  // Also `serde::as_btc`.
    ///     amount: SignedAmount,
    /// }
    /// # }
    /// ```
    #[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct SignedAmount(i64);

    impl SignedAmount {
        /// Constructs a new [`SignedAmount`] with satoshi precision and the given number of satoshis.
        ///
        /// Caller to guarantee that `satoshi` is within valid range.
        ///
        /// See [`Self::MIN`] and [`Self::MAX`].
        pub const fn from_sat_unchecked(satoshi: i64) -> SignedAmount { SignedAmount(satoshi) }

        /// Gets the number of satoshis in this [`SignedAmount`].
        ///
        /// # Examples
        ///
        /// ```
        /// # use bitcoin_units::SignedAmount;
        /// assert_eq!(SignedAmount::ONE_BTC.to_sat(), 100_000_000);
        /// ```
        pub const fn to_sat(self) -> i64 { self.0 }
    }
}
#[doc(inline)]
pub use encapsulate::SignedAmount;

impl SignedAmount {
    /// The zero amount.
    pub const ZERO: Self = SignedAmount::from_sat_unchecked(0);
    /// Exactly one satoshi.
    pub const ONE_SAT: Self = SignedAmount::from_sat_unchecked(1);
    /// Exactly one bitcoin.
    pub const ONE_BTC: Self = SignedAmount::from_sat_unchecked(100_000_000);
    /// Exactly fifty bitcoin.
    pub const FIFTY_BTC: Self = SignedAmount::from_sat_unchecked(50 * 100_000_000);
    /// The maximum value allowed as an amount. Useful for sanity checking.
    pub const MAX_MONEY: Self = SignedAmount::from_sat_unchecked(21_000_000 * 100_000_000);
    /// The minimum value of an amount.
    pub const MIN: Self = SignedAmount::from_sat_unchecked(-21_000_000 * 100_000_000);
    /// The maximum value of an amount.
    pub const MAX: Self = SignedAmount::MAX_MONEY;

    /// Constructs a new [`SignedAmount`] from the given number of satoshis.
    ///
    /// # Errors
    ///
    /// If `satoshi` is outside of valid range (see [`Self::MAX_MONEY`]).
    ///
    /// # Examples
    ///
    /// ```
    /// # use bitcoin_units::{amount, SignedAmount};
    /// # let sat = -100_000;
    /// let amount = SignedAmount::from_sat(sat)?;
    /// assert_eq!(amount.to_sat(), sat);
    /// # Ok::<_, amount::OutOfRangeError>(())
    /// ```
    pub const fn from_sat(satoshi: i64) -> Result<SignedAmount, OutOfRangeError> {
        if satoshi < Self::MIN.to_sat() {
            Err(OutOfRangeError { is_signed: true, is_greater_than_max: false })
        } else if satoshi > Self::MAX_MONEY.to_sat() {
            Err(OutOfRangeError { is_signed: true, is_greater_than_max: true })
        } else {
            Ok(SignedAmount::from_sat_unchecked(satoshi))
        }
    }

    /// Converts from a value expressing a decimal number of bitcoin to a [`SignedAmount`].
    ///
    /// # Errors
    ///
    /// If the amount is too big (positive or negative) or too precise.
    ///
    /// Please be aware of the risk of using floating-point numbers.
    ///
    /// # Examples
    ///
    /// ```
    /// # use bitcoin_units::{amount, SignedAmount};
    /// let amount = SignedAmount::from_btc(-0.01)?;
    /// assert_eq!(amount.to_sat(), -1_000_000);
    /// # Ok::<_, amount::ParseAmountError>(())
    /// ```
    #[cfg(feature = "alloc")]
    pub fn from_btc(btc: f64) -> Result<SignedAmount, ParseAmountError> {
        Self::from_float_in(btc, Denomination::Bitcoin)
    }

    /// Converts from a value expressing a whole number of bitcoin to a [`SignedAmount`].
    ///
    /// # Errors
    ///
    /// If `whole_bitcoin` is greater than `21_000_000`.
    #[allow(clippy::missing_panics_doc)]
    pub fn from_int_btc<T: Into<i32>>(whole_bitcoin: T) -> Result<SignedAmount, OutOfRangeError> {
        SignedAmount::from_int_btc_const(whole_bitcoin.into())
    }

    /// Converts from a value expressing a whole number of bitcoin to a [`SignedAmount`]
    /// in const context.
    ///
    /// # Errors
    ///
    /// If `whole_bitcoin` is greater than `21_000_000`.
    #[allow(clippy::missing_panics_doc)]
    pub const fn from_int_btc_const(whole_bitcoin: i32) -> Result<SignedAmount, OutOfRangeError> {
        let btc = whole_bitcoin as i64; // Can't call `into` in const context.
        match btc.checked_mul(100_000_000) {
            Some(amount) => SignedAmount::from_sat(amount),
            None => panic!("cannot overflow in i64"),
        }
    }

    /// Parses a decimal string as a value in the given [`Denomination`].
    ///
    /// Note: This only parses the value string. If you want to parse a string
    /// containing the value with denomination, use [`FromStr`].
    ///
    /// # Errors
    ///
    /// If the amount is too big (positive or negative) or too precise.
    pub fn from_str_in(s: &str, denom: Denomination) -> Result<SignedAmount, ParseAmountError> {
        match parse_signed_to_satoshi(s, denom).map_err(|error| error.convert(true))? {
            // (negative, amount)
            (false, sat) if sat > SignedAmount::MAX.to_sat() as u64 => Err(ParseAmountError(
                ParseAmountErrorInner::OutOfRange(OutOfRangeError::too_big(true)),
            )),
            (false, sat) => Ok(SignedAmount::from_sat_unchecked(sat as i64)), // Cast ok, value in this arm does not wrap.
            (true, sat) if sat > SignedAmount::MIN.to_sat().unsigned_abs() => Err(
                ParseAmountError(ParseAmountErrorInner::OutOfRange(OutOfRangeError::too_small())),
            ),
            (true, sat) => Ok(SignedAmount::from_sat_unchecked(-(sat as i64))), // Cast ok, value in this arm does not wrap.
        }
    }

    /// Parses amounts with denomination suffix as produced by [`Self::to_string_with_denomination`]
    /// or with [`fmt::Display`].
    ///
    /// If you want to parse only the amount without the denomination, use [`Self::from_str_in`].
    ///
    /// # Errors
    ///
    /// If the amount is too big (positive or negative) or too precise.
    ///
    /// # Examples
    ///
    /// ```
    /// # use bitcoin_units::{amount, SignedAmount};
    /// let amount = SignedAmount::from_str_with_denomination("0.1 BTC")?;
    /// assert_eq!(amount, SignedAmount::from_sat(10_000_000)?);
    /// # Ok::<_, amount::ParseError>(())
    /// ```
    pub fn from_str_with_denomination(s: &str) -> Result<SignedAmount, ParseError> {
        let (amt, denom) = split_amount_and_denomination(s)?;
        Self::from_str_in(amt, denom).map_err(Into::into)
    }

    /// Expresses this [`SignedAmount`] as a floating-point value in the given [`Denomination`].
    ///
    /// Please be aware of the risk of using floating-point numbers.
    ///
    /// # Examples
    ///
    /// ```
    /// # use bitcoin_units::amount::{self, SignedAmount, Denomination};
    /// let amount = SignedAmount::from_sat(100_000)?;
    /// assert_eq!(amount.to_float_in(Denomination::Bitcoin), 0.001);
    /// # Ok::<_, amount::OutOfRangeError>(())
    /// ```
    #[cfg(feature = "alloc")]
    #[allow(clippy::missing_panics_doc)]
    pub fn to_float_in(self, denom: Denomination) -> f64 {
        self.to_string_in(denom).parse::<f64>().unwrap()
    }

    /// Expresses this [`SignedAmount`] as a floating-point value in Bitcoin.
    ///
    /// Please be aware of the risk of using floating-point numbers.
    ///
    /// # Examples
    ///
    /// ```
    /// # use bitcoin_units::amount::{self, SignedAmount, Denomination};
    /// let amount = SignedAmount::from_sat(100_000)?;
    /// assert_eq!(amount.to_btc(), amount.to_float_in(Denomination::Bitcoin));
    /// # Ok::<_, amount::OutOfRangeError>(())
    /// ```
    #[cfg(feature = "alloc")]
    pub fn to_btc(self) -> f64 { self.to_float_in(Denomination::Bitcoin) }

    /// Converts this [`SignedAmount`] in floating-point notation in the given [`Denomination`].
    ///
    /// # Errors
    ///
    /// If the amount is too big (positive or negative) or too precise.
    ///
    /// Please be aware of the risk of using floating-point numbers.
    #[cfg(feature = "alloc")]
    pub fn from_float_in(
        value: f64,
        denom: Denomination,
    ) -> Result<SignedAmount, ParseAmountError> {
        // This is inefficient, but the safest way to deal with this. The parsing logic is safe.
        // Any performance-critical application should not be dealing with floats.
        Self::from_str_in(&value.to_string(), denom)
    }

    /// Constructs a new object that implements [`fmt::Display`] in the given [`Denomination`].
    ///
    /// This function is useful if you do not wish to allocate. See also [`Self::to_string_in`].
    ///
    /// # Examples
    ///
    /// ```
    /// # use bitcoin_units::amount::{self, SignedAmount, Denomination};
    /// # use std::fmt::Write;
    /// let amount = SignedAmount::from_sat(10_000_000)?;
    /// let mut output = String::new();
    /// let _ = write!(&mut output, "{}", amount.display_in(Denomination::Bitcoin));
    /// assert_eq!(output, "0.1");
    /// # Ok::<_, amount::OutOfRangeError>(())
    /// ```
    #[must_use]
    pub fn display_in(self, denomination: Denomination) -> Display {
        Display {
            sats_abs: self.unsigned_abs().to_sat(),
            is_negative: self.is_negative(),
            style: DisplayStyle::FixedDenomination { denomination, show_denomination: false },
        }
    }

    /// Constructs a new object that implements [`fmt::Display`] dynamically selecting
    /// [`Denomination`].
    ///
    /// This will use BTC for values greater than or equal to 1 BTC and satoshis otherwise. To
    /// avoid confusion the denomination is always shown.
    #[must_use]
    pub fn display_dynamic(self) -> Display {
        Display {
            sats_abs: self.unsigned_abs().to_sat(),
            is_negative: self.is_negative(),
            style: DisplayStyle::DynamicDenomination,
        }
    }

    /// Returns a formatted string representing this [`SignedAmount`] in the given [`Denomination`].
    ///
    /// Returned string does not include the denomination.
    ///
    /// # Examples
    ///
    /// ```
    /// # use bitcoin_units::amount::{self, SignedAmount, Denomination};
    /// let amount = SignedAmount::from_sat(10_000_000)?;
    /// assert_eq!(amount.to_string_in(Denomination::Bitcoin), "0.1");
    /// # Ok::<_, amount::OutOfRangeError>(())
    /// ```
    #[cfg(feature = "alloc")]
    pub fn to_string_in(self, denom: Denomination) -> String { self.display_in(denom).to_string() }

    /// Returns a formatted string representing this [`SignedAmount`] in the given [`Denomination`],
    /// suffixed with the abbreviation for the denomination.
    ///
    /// # Examples
    ///
    /// ```
    /// # use bitcoin_units::amount::{self, SignedAmount, Denomination};
    /// let amount = SignedAmount::from_sat(10_000_000)?;
    /// assert_eq!(amount.to_string_with_denomination(Denomination::Bitcoin), "0.1 BTC");
    /// # Ok::<_, amount::OutOfRangeError>(())
    /// ```
    #[cfg(feature = "alloc")]
    pub fn to_string_with_denomination(self, denom: Denomination) -> String {
        self.display_in(denom).show_denomination().to_string()
    }

    /// Gets the absolute value of this [`SignedAmount`].
    ///
    /// This function never overflows or panics, unlike `i64::abs()`.
    #[must_use]
    pub const fn abs(self) -> SignedAmount {
        // `i64::abs()` can never overflow because SignedAmount::MIN == -MAX_MONEY.
        match Self::from_sat(self.to_sat().abs()) {
            Ok(amount) => amount,
            Err(_) => panic!("a positive signed amount is always valid"),
        }
    }

    /// Gets the absolute value of this [`SignedAmount`] returning [`Amount`].
    #[must_use]
    #[allow(clippy::missing_panics_doc)]
    pub fn unsigned_abs(self) -> Amount {
        self.abs().to_unsigned().expect("a positive signed amount is always valid")
    }

    /// Returns a number representing sign of this [`SignedAmount`].
    ///
    /// - `0` if the amount is zero
    /// - `1` if the amount is positive
    /// - `-1` if the amount is negative
    #[must_use]
    pub fn signum(self) -> i64 { self.to_sat().signum() }

    /// Checks if this [`SignedAmount`] is positive.
    ///
    /// Returns `true` if this [`SignedAmount`] is positive and `false` if
    /// this [`SignedAmount`] is zero or negative.
    pub fn is_positive(self) -> bool { self.to_sat().is_positive() }

    /// Checks if this [`SignedAmount`] is negative.
    ///
    /// Returns `true` if this [`SignedAmount`] is negative and `false` if
    /// this [`SignedAmount`] is zero or positive.
    pub fn is_negative(self) -> bool { self.to_sat().is_negative() }

    /// Returns the absolute value of this [`SignedAmount`].
    ///
    /// Consider using `unsigned_abs` which is often more practical.
    ///
    /// Returns [`None`] if overflow occurred. (`self == i64::MIN`)
    #[must_use]
    #[deprecated(since = "TBD", note = "Never returns none, use `abs()` instead")]
    #[allow(clippy::unnecessary_wraps)] // To match stdlib function definition.
    pub const fn checked_abs(self) -> Option<SignedAmount> { Some(self.abs()) }

    /// Checked addition.
    ///
    /// Returns [`None`] if the sum is above [`SignedAmount::MAX`] or below [`SignedAmount::MIN`].
    #[must_use]
    pub const fn checked_add(self, rhs: SignedAmount) -> Option<SignedAmount> {
        // No `map()` in const context.
        match self.to_sat().checked_add(rhs.to_sat()) {
            Some(res) => match SignedAmount::from_sat(res) {
                Ok(amount) => Some(amount),
                Err(_) => None,
            },
            None => None,
        }
    }

    /// Checked subtraction.
    ///
    /// Returns [`None`] if the difference is above [`SignedAmount::MAX`] or below
    /// [`SignedAmount::MIN`].
    #[must_use]
    pub const fn checked_sub(self, rhs: SignedAmount) -> Option<SignedAmount> {
        // No `map()` in const context.
        match self.to_sat().checked_sub(rhs.to_sat()) {
            Some(res) => match Self::from_sat(res) {
                Ok(amount) => Some(amount),
                Err(_) => None,
            },
            None => None,
        }
    }

    /// Checked multiplication.
    ///
    /// Returns [`None`] if the product is above [`SignedAmount::MAX`] or below
    /// [`SignedAmount::MIN`].
    #[must_use]
    pub const fn checked_mul(self, rhs: i64) -> Option<SignedAmount> {
        // No `map()` in const context.
        match self.to_sat().checked_mul(rhs) {
            Some(res) => match Self::from_sat(res) {
                Ok(amount) => Some(amount),
                Err(_) => None,
            },
            None => None,
        }
    }

    /// Checked integer division.
    ///
    /// Be aware that integer division loses the remainder if no exact division can be made.
    ///
    /// Returns [`None`] if overflow occurred.
    #[must_use]
    pub const fn checked_div(self, rhs: i64) -> Option<SignedAmount> {
        // No `map()` in const context.
        match self.to_sat().checked_div(rhs) {
            Some(res) => match Self::from_sat(res) {
                Ok(amount) => Some(amount),
                Err(_) => None, // Unreachable because of checked_div above.
            },
            None => None,
        }
    }

    /// Checked remainder.
    ///
    /// Returns [`None`] if overflow occurred.
    #[must_use]
    pub const fn checked_rem(self, rhs: i64) -> Option<SignedAmount> {
        // No `map()` in const context.
        match self.to_sat().checked_rem(rhs) {
            Some(res) => match Self::from_sat(res) {
                Ok(amount) => Some(amount),
                Err(_) => None, // Unreachable because of checked_rem above.
            },
            None => None,
        }
    }

    /// Subtraction that doesn't allow negative [`SignedAmount`]s.
    ///
    /// Returns [`None`] if either `self`, `rhs` or the result is strictly negative.
    #[must_use]
    pub fn positive_sub(self, rhs: SignedAmount) -> Option<SignedAmount> {
        if self.is_negative() || rhs.is_negative() || rhs > self {
            None
        } else {
            self.checked_sub(rhs)
        }
    }

    /// Converts to an unsigned amount.
    ///
    /// # Errors
    ///
    /// If the amount is negative.
    #[allow(clippy::missing_panics_doc)]
    pub fn to_unsigned(self) -> Result<Amount, OutOfRangeError> {
        if self.is_negative() {
            Err(OutOfRangeError::negative())
        } else {
            // Cast ok, checked not negative above.
            Ok(Amount::from_sat(self.to_sat() as u64)
                .expect("a positive signed amount is always valid"))
        }
    }
}

impl default::Default for SignedAmount {
    fn default() -> Self { SignedAmount::ZERO }
}

impl fmt::Debug for SignedAmount {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "SignedAmount({} SAT)", self.to_sat())
    }
}

// No one should depend on a binding contract for Display for this type.
// Just using Bitcoin denominated string.
impl fmt::Display for SignedAmount {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(&self.display_in(Denomination::Bitcoin).show_denomination(), f)
    }
}

impl FromStr for SignedAmount {
    type Err = ParseError;

    /// Parses a string slice where the slice includes a denomination.
    ///
    /// If the returned value would be zero or negative zero, then no denomination is required.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let result = Self::from_str_with_denomination(s);

        match result {
            Err(ParseError(ParseErrorInner::MissingDenomination(_))) => {
                let d = Self::from_str_in(s, Denomination::Satoshi);

                if d == Ok(Self::ZERO) {
                    Ok(Self::ZERO)
                } else {
                    result
                }
            }
            _ => result,
        }
    }
}

impl From<Amount> for SignedAmount {
    fn from(value: Amount) -> Self {
        let v = value.to_sat() as i64; // Cast ok, signed amount and amount share positive range.
        Self::from_sat(v).expect("all amounts are valid signed amounts")
    }
}

#[cfg(feature = "arbitrary")]
impl<'a> Arbitrary<'a> for SignedAmount {
    fn arbitrary(u: &mut Unstructured<'a>) -> arbitrary::Result<Self> {
        let sats = u.int_in_range(Self::MIN.to_sat()..=Self::MAX.to_sat())?;
        Ok(Self::from_sat(sats).expect("range is valid"))
    }
}
