use crate::util::wire_string_enum;

wire_string_enum! {
    /// A Bloque asset symbol, e.g. `"DUSD/6"` (US Dollar, 6 decimals).
    /// Amounts for an asset are always raw integer strings scaled by its
    /// decimals — `"10000000"` of `DUSD/6` is 10.000000 USD.
    pub enum Asset {
        Dusd6 => "DUSD/6",
        Copb6 => "COPB/6",
        Copm2 => "COPM/2",
        Ksm12 => "KSM/12",
    }
}
