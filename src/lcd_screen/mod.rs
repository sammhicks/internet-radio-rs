use crate::message::PlayerState;
use anyhow::{Context, Result};
use clerk::{DataPins4Lines, Pins};
use std::fs::File;
use std::io::prelude::*;
use tokio::{runtime, sync::watch};
extern crate hex;

mod character_pattern;

enum LCDLineNumbers {
    Line1,
    Line2,
    Line3,
    Line4,
}

impl LCDLineNumbers {
    const NUM_CHARACTERS_PER_LINE: u8 = 20; //the display is visually 20 * 4 characters
    const ROW_OFFSET: u8 = 0x40; //specified by the chip

    fn offset(self) -> u8 {
        match self {
            LCDLineNumbers::Line1 => 0,
            LCDLineNumbers::Line2 => Self::ROW_OFFSET,
            LCDLineNumbers::Line3 => Self::NUM_CHARACTERS_PER_LINE,
            LCDLineNumbers::Line4 => Self::ROW_OFFSET + Self::NUM_CHARACTERS_PER_LINE,
        }
    }
}

struct FakeLine;

impl clerk::DisplayHardwareLayer for FakeLine {
    fn set_level(&self, _level: clerk::Level) {}
    fn set_direction(&self, _direction: clerk::Direction) {}
    fn get_value(&self) -> u8 {
        0
    }
}

struct Line {
    handle: gpio_cdev::LineHandle,
}

impl clerk::DisplayHardwareLayer for Line {
    fn set_level(&self, level: clerk::Level) {
        self.handle
            .set_value(match level {
                clerk::Level::Low => 0,
                clerk::Level::High => 1,
            })
            .unwrap();
    }
    fn set_direction(&self, _direction: clerk::Direction) {}

    fn get_value(&self) -> u8 {
        0
    }
}

struct Delay;

impl clerk::Delay for Delay {
    const ADDRESS_SETUP_TIME: u16 = 60;
    const ENABLE_PULSE_WIDTH: u16 = 300; // 300ns in the spec sheet 450;
    const DATA_HOLD_TIME: u16 = 10; // 10ns in the spec sheet  20;
    const COMMAND_EXECUTION_TIME: u16 = 37;

    fn delay_ns(ns: u16) {
        std::thread::sleep(std::time::Duration::from_nanos(u64::from(ns)));
    }
}

fn get_line(chip: &mut gpio_cdev::Chip, offset: u32, consumer: &'static str) -> Result<Line> {
    let handle = chip
        .get_line(offset)
        .with_context(|| format!("Failed to get GPIO pin for {:?}", consumer))?
        .request(gpio_cdev::LineRequestFlags::OUTPUT, 0, consumer)
        .with_context(|| format!("GPIO pin for {:?} already in use. Are running another copy of the program elsewhere?", consumer))?;
    Ok(Line { handle })
}
#[derive(Debug, serde::Deserialize)]
struct PinDeclarations {
    rs: u32,     // Register Select
    enable: u32, // Also known as strobe and clock
    data4: u32,
    data5: u32,
    data6: u32,
    data7: u32,
}

impl PinDeclarations {
    fn create_display(
        self,
        chip: &mut gpio_cdev::Chip,
    ) -> Result<
        clerk::Display<
            clerk::ParallelConnection<
                Line,
                FakeLine,
                Line,
                clerk::DataPins4Lines<Line, Line, Line, Line>,
                Delay,
            >,
            clerk::DefaultLines,
        >,
    > {
        let register_select = get_line(chip, self.rs, "register_select")?;
        let read = FakeLine;
        let enable = get_line(chip, self.enable, "enable")?;
        let data4 = get_line(chip, self.data4, "data4")?;
        let data5 = get_line(chip, self.data5, "data5")?;
        let data6 = get_line(chip, self.data6, "data6")?;
        let data7 = get_line(chip, self.data7, "data7")?;

        let pins = Pins {
            register_select,
            read,
            enable,
            data: DataPins4Lines {
                data4,
                data5,
                data6,
                data7,
            },
        };

        let lcd = clerk::Display::<_, clerk::DefaultLines>::new(pins.into_connection::<Delay>());

        lcd.init(clerk::FunctionSetBuilder::default().set_line_number(clerk::LineNumber::Two)); // screen has 4 lines, but electrically, only 2
        std::thread::sleep(std::time::Duration::from_millis(3)); // with this line commented out, screen goes blank, and cannot be written to subsequently
                                                                 // 1.5 ms is marginal as 1.2ms does not work.

        lcd.set_display_control(
            clerk::DisplayControlBuilder::default() // defaults are display on cursor off blinking off ie cursor is an underscore
                .set_cursor(clerk::CursorState::On), // normally we want the cursor off
        ); //no extra delay needed here

        lcd.clear();
        std::thread::sleep(std::time::Duration::from_millis(2)); // if this line is commented out, garbage or nothing appears. 1ms is marginal

        Ok(lcd)
    }
}

pub fn run(handle: &runtime::Handle, player_state: watch::Receiver<PlayerState>) {
    if let Err(err) = try_run(handle, player_state) {
        log::error!("{:?}", err);
    }
}

fn try_run(handle: &runtime::Handle, mut player_state: watch::Receiver<PlayerState>) -> Result<()> {
    log::info!("Hello World from lcd_screen");

    let pins_src = std::fs::read_to_string("/boot/wiring_pins.toml")
        .context("Failed to read GPIO pin declarations file")?;
    let pins: PinDeclarations =
        toml::from_str(&pins_src).context("Failed to parse GPIO pin declarations file")?;

    log::info!("{:?}", pins);

    let mut chip =
        gpio_cdev::Chip::new("/dev/gpiochip0").context("Failed to open GPIO character device")?; // no delay needed here
    let mut lcd = pins.create_display(&mut chip)?;

    lcd.seek_cgram(clerk::SeekFrom::Home(0)); // specify we want to write to the character generator in position 0. Must be a multiple of 8 if we want to start at the start of character

    for character_bitmap in &character_pattern::BITMAPS {
        for row in character_bitmap {
            lcd.write(*row);
        }
    }

    lcd.seek(clerk::SeekFrom::Home(0)); // specify we want to write characters to be output, starting at position 0

    lcd.seek(clerk::SeekFrom::Home(LCDLineNumbers::Line2.offset())); //TBD not the correct line but good for debug purposes
    for octet in format!("CPU Temp {} C", get_cpu_temperature()).chars() {
        lcd.write(octet as u8)
    }

    println!("local IP address {}\r", get_local_ip_address());

    println!("router address {:?}\r", get_gateway_address());

    while let Some(next_state) = handle.block_on(player_state.recv()) {
        log::info!("next_state.current_track{:?}", next_state.current_track);

        lcd.seek(clerk::SeekFrom::Home(
            LCDLineNumbers::Line1.offset() + LCDLineNumbers::NUM_CHARACTERS_PER_LINE - 7,
        ));

        let message = if next_state.pipeline_state.is_playing() {
            format!("Vol{:>4.7}", next_state.volume)
        } else {
            format!("{:<7.7}", next_state.pipeline_state.to_string()) //if we use  next_state.pipeline_state.to_string() without the .to_string, the result can be less than 7 characters long
        };

        for unicode_character in message.chars() {
            if unicode_character < '~' {
                // characters lower than ~ are handled by the built-in character set
                lcd.write(unicode_character as u8)
            } else {
                let ascii_character_bytes = match unicode_character {
                    'é' => &[5], // e accute fifth bespoke character defined starting with the zeroeth bespoke character
                    'è' => &[6], // e grave
                    'à' => &[7], // a grave
                    'ä' => &[0xE1], // a umlaut            // see look up table in GDM2004D.pdf page 9/9
                    'ñ' => &[0xEE], // n tilde
                    'ö' => &[0xEF], // o umlaut
                    'ü' => &[0xF5], // u umlaut
                    'π' => &[0xE4], // pi
                    'µ' => &[0xF7], // mu
                    '~' => &[0xF3], // cannot display tilde using the standard character set in GDM2004D.pdf. This is the best we can do.
                    '' => &[0xFF], // <Control>  = 0x80 replaced by splodge
                    _ => unidecode::unidecode_char(unicode_character).as_bytes(),
                };
                for octet in ascii_character_bytes {
                    lcd.write(*octet);
                }
            }
        }

        println!("\rbuf Rx by screen {}\r", next_state.buffering);
        let trimmed_buffer = next_state.buffering.min(99); //0 to 100 is 101 values, & the screen only handles 100 values, so trim downwards

        #[allow(clippy::cast_possible_wrap)]
        let scaled_buffer = (trimmed_buffer / 5) as i8; //the characters have 5 columns

        lcd.seek(clerk::SeekFrom::Home(LCDLineNumbers::Line4.offset() + 0));

        for _count in 0..scaled_buffer {
            lcd.write(' ' as u8); //first write space in all the character positions before the cursor
        }
        lcd.write((trimmed_buffer % 5) as u8); //then write the apppriate cursor character in the next position
        for _count in scaled_buffer + 1..20 {
            lcd.write(' ' as u8); //then clear the rest of the line
        }
    }

    lcd.clear();
    std::thread::sleep(std::time::Duration::from_millis(2)); // if this line is commented out, garbage or nothing appears. 1ms is marginal

    lcd.seek(clerk::SeekFrom::Home(LCDLineNumbers::Line1.offset()));

    for character in "Program shut down".chars() {
        lcd.write(character as u8);
    }

    Ok(())
}
fn get_cpu_temperature() -> i32 {
    let mut file = File::open("/sys/class/thermal/thermal_zone0/temp").unwrap_or_else(|error| {
        panic!(
            "Problem opening the CPU temperature pseudo-file: {:?}",
            error
        );
    });

    let mut cpu_temperature = String::new();

    match file.read_to_string(&mut cpu_temperature) {
        Err(why) => panic!("couldn't read the temperature from the pseduo file {}", why),
        Ok(_file_size) => {
            let milli_temp: i32 = cpu_temperature //cpu_temperature contains the temperature in milli-C and a line terminator
                .trim() //to get rid of the terminator
                .parse()
                .expect("CPU temperature was non-numeric");
            return milli_temp / 1000; //divide by 1000 to convert to C from milli-C and return the temperature
        }
    };
}
fn get_local_ip_address() -> String {
    let mut return_value: String = String::from("bad Local IP address");
    for iface in pnet::datalink::interfaces() {
        if iface.is_up() && !iface.is_loopback() && iface.ips.len() > 0 {
            // this if statement filters off the loopback address & addresses that do not have an IP address
            for ipaddr in &iface.ips {
                let ip4addr = match ipaddr {
                    pnet::ipnetwork::IpNetwork::V4(addr) => addr.ip(), // filters off the "/24" at the end of the IP address
                    pnet::ipnetwork::IpNetwork::V6(_) => continue,
                };
                return_value = ip4addr.to_string();
            }
        }
    }
    return_value
}
pub fn get_gateway_address() -> Result<String, String> {
    /* example contents of file are shown below. If the destination is 0, the address given as the gateway is what we are looking for, but byte reversed
       Iface   Destination     Gateway         Flags   RefCnt  Use     Metric  Mask            MTU     Window  IRTT
       wlan0   00000000        0200A8C0        0003    0       0       303     00000000        0       0       0
       wlan0   0000A8C0        00000000        0001    0       0       303     00FFFFFF        0       0       0
    */
    if let Ok(file) = File::open("/proc/net/route") {
        let buffered_file = std::io::BufReader::new(file);
        for one_line_or_error in buffered_file.lines() {
            if let Ok(one_line) = one_line_or_error {
                let line_one_elements: Vec<&str> = one_line.split('\t').collect();
                if line_one_elements.len() > 100 && line_one_elements[1] == "00000000" {
                    match hex::decode(line_one_elements[2]) {
                        Ok(gateway_address_vec) => {
                            let gateway_address = format!(
                                "{}.{}.{}.{}",
                                gateway_address_vec[3],
                                gateway_address_vec[2],
                                gateway_address_vec[1],
                                gateway_address_vec[0]
                            );
                            return Ok(gateway_address);
                        }
                        Err(_) => return Err("Router address not hex digits".to_string()),
                    }
                }
            } else {
                return Err("Failed to read the router address".to_string());
            }
        }
        return Err("Could  not find the router address".to_string());
    } else {
        return Err("Couldn't open the file to get the address of the router".to_string());
    }
}
