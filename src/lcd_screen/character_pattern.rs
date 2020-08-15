pub const BITMAPS: [[u8; 8]; 8] = [
    [
        0b10000, //pattern for topmost row. the 1 specifies that the top left pixel is on, the other zeroes specify that the other topmost pixels are off
        0b10000, //
        0b10000, //
        0b10000, //
        0b10000, //
        0b10000, //
        0b10000, //
        0b11111,
    ], //
    [
        0b01000, //
        0b01000, //
        0b01000, //
        0b01000, //
        0b01000, //
        0b01000, //
        0b01000, //
        0b11111,
    ],
    [
        0b00100, //
        0b00100, //
        0b00100, //
        0b00100, //
        0b00100, //
        0b00100, //
        0b00100, //
        0b11111,
    ],
    [
        0b00010, //
        0b00010, //
        0b00010, //
        0b00010, //
        0b00010, //
        0b00010, //
        0b00010, //
        0b11111,
    ],
    [
        0b00001, //
        0b00001, //
        0b00001, //
        0b00001, //
        0b00001, //
        0b00001, //
        0b00001, //
        0b11111,
    ],
    [
        0b01100, // pattern for topmost row for e accute
        0b10000, // this pattern specifies that the left-most bit is on, & the other 4 are off on the top but one row.
        0b01110, //
        0b10001, //
        0b11111, //
        0b10000, //
        0b01110, //
        0b00000, // bottom row, which is expected to be all zeros
    ],
    [
        0b00110, // e grave pattern
        0b00001, //
        0b01110, //
        0b10001, //
        0b11111, //
        0b10000, //
        0b01110, //
        0b00000,
    ],
    [
        0b00110, // a grave pattern
        0b00001, //
        0b01110, //
        0b00001, //
        0b01111, //
        0b10001, //
        0b01111, //
        0b00000,
    ],
];
