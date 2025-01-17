use byteorder::{ByteOrder, LittleEndian};
use clap::{value_parser, Arg, ArgAction, Command};
use picontrol::{get_module_name, is_module_connected, SDeviceInfo, SPIValue};

use std::str::FromStr;

#[derive(Debug, Clone, Copy)]
enum Formats {
    Decimal,
    Hex,
    Binary,
}

impl FromStr for Formats {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "d" => Ok(Formats::Decimal),
            "h" => Ok(Formats::Hex),
            "b" => Ok(Formats::Binary),
            _ => Err("no match"),
        }
    }
}

fn create_clap_app() -> clap::Command {
    Command::new("pitestrs")
        .version("1.0")
        .about("pitest command line written in Rust")
        .arg(
            Arg::new("device-list")
                .short('l')
                .action(ArgAction::SetTrue)
                .help("Shows the device list"),
        )
        .arg(
            Arg::new("reset")
                .short('x')
                .long("reset")
                .action(ArgAction::SetTrue)
                .help("Resets the piControl driver"),
        )
        .arg(
            Arg::new("firmware-update")
                .short('f')
                .action(ArgAction::SetTrue)
                .help("Updates the firmware of a module"),
        )
        .arg(
            Arg::new("image-source")
                .short('s')
                .help("The process image dumped file path, if empty the default is used"),
        )
        .subcommand(
            Command::new("read")
                .about("Reads a variable")
                .arg(
                    Arg::new("variable-name")
                        .short('n')
                        .help("the variable name"),
                )
                .arg(
                    Arg::new("variable-format")
                        .short('f')
                        .default_value("d")
                        .value_parser(value_parser!(Formats))
                        .required(true)
                        .help("the variable format"),
                ),
        )
        .subcommand(
            Command::new("write")
                .about("Writes a variable")
                .arg(
                    Arg::new("variable-name")
                        .short('n')
                        .help("the variable name"),
                )
                .arg(
                    Arg::new("variable-value")
                        .short('v')
                        .required(true)
                        .value_parser(value_parser!(u32))
                        .help("the variable value"),
                ),
        )
        .subcommand(
            Command::new("dump")
                .about("Writes the process image to a file")
                .arg(
                    Arg::new("file-path")
                        .short('f')
                        .help("the file path")
                        .default_value("revpi_proc_img.bin"),
                ),
        )
}

fn main() {
    let matches = create_clap_app().get_matches();

    // this implements the drop trait, cleans up memory after going out of scope
    let mut picontrol = picontrol::RevPiControl::new();

    if matches.contains_id("image-source") {
        let m = matches.get_one::<String>("image-source").unwrap();
        picontrol = picontrol::RevPiControl::new_at(m);
    }

    if let Err(err) = picontrol.open() {
        println!("open file error: {}", err);
        return;
    }

    if matches.get_flag("reset") {
        if let Err(err) = picontrol.reset() {
            println!("reset error: {}", err);
        }
        return;
    }

    if matches.get_flag("device-list") {
        match picontrol.get_device_info_list() {
            Err(err) => {
                println!("ls error: {}", err);
                return;
            }
            Ok(list) => {
                show_device_list(list);
                return;
            }
        }
    }

    if matches.get_flag("firmware-update") {
        println!("Value for config");
    }

    if let Some(matches) = matches.subcommand_matches("read") {
        // "$ myapp test" was run
        if let Some(varname) = matches.get_one::<String>("variable-name") {
            let format = *matches
                .get_one::<Formats>("variable-format")
                .expect("invalid read format");

            println!("Value for variable name: {}", varname);
            read_variable_value(&mut picontrol, varname, format, false).unwrap_or_else(|err| {
                println!("error reading variable: {}", err);
                false
            });
        } else {
            println!("no variable specified");
        }
    }

    if let Some(matches) = matches.subcommand_matches("write") {
        if let Some(varname) = matches.get_one::<String>("variable-name") {
            println!("Value for variable name: {}", varname);

            let value = *matches
                .get_one::<u32>("variable-value")
                .expect("invalid write value");

            write_variable_value(&mut picontrol, varname, value).unwrap_or_else(|err| {
                println!("error writing variable: {}", err);
                false
            });
        } else {
            println!("no variable specified");
        }
    }

    if let Some(matches) = matches.subcommand_matches("dump") {
        if let Some(fp) = matches.get_one::<String>("file-path") {
            if let Err(err) = picontrol.dump(fp) {
                println!("dump error: {}", err);
            }
        } else {
            println!("no file path specified");
        }
    }
}

fn read_variable_value(
    picontrol: &mut picontrol::RevPiControl,
    name: &str,
    // cyclic: bool,
    format: Formats,
    quiet: bool,
) -> Result<bool, Box<dyn std::error::Error>> {
    let mut spivalue: SPIValue = SPIValue {
        ..Default::default()
    };

    let spivariable = picontrol.get_variable_info(name)?;

    if spivariable.i16uLength == 1 {
        spivalue.i16uAddress = spivariable.i16uAddress;
        spivalue.i8uBit = spivariable.i8uBit;

        picontrol.get_bit_value(&mut spivalue)?;
        if !quiet {
            println!("Bit value: {}", spivalue.i8uValue);
        } else {
            println!("{}", spivalue.i8uValue);
        }
    } else {
        let remainder = spivariable.i16uLength % 8;
        if remainder != 0 {
            return Err(From::from(format!(
                "could not read variable {}. Internal Error",
                name
            )));
        }
        let size = spivariable.i16uLength / 8;

        match spivariable.i16uLength {
            8 | 16 | 32 => {
                let data: Vec<u8> =
                    picontrol.read(spivariable.i16uAddress as u64, size as usize)?;
                println!(
                    "read from address {}, byte size {}, data: {:x?}",
                    spivariable.i16uAddress, size, data
                );
                let u32_value = match spivariable.i16uLength {
                    8 => data[0] as u32,
                    16 => LittleEndian::read_u16(&data) as u32,
                    32 => LittleEndian::read_u32(&data) as u32,
                    _ => {
                        return Err(From::from(format!(
                            "invalid length for variable {}. Internal Error",
                            name
                        )));
                    }
                };

                match format {
                    Formats::Hex => {
                        if !quiet {
                            println!(
                                "{} byte-value of {}: {:x?} hex bytes (={} dec)",
                                size,
                                name,
                                data.as_ref() as &[u8],
                                u32_value
                            );
                        } else {
                            println!("{:x}", u32_value);
                        }
                    }
                    Formats::Binary => {
                        if !quiet {
                            println!("{} byte value of {}: ", size, name);
                        }

                        let bn = picontrol::num_to_bytes(u32_value as u64, 32).unwrap();
                        println!("binary value: {:x?}", bn);
                    }
                    _ => {
                        if !quiet {
                            println!(
                                "{} byte-value of {}: {} dec (={:x?} hex bytes)",
                                size,
                                name,
                                u32_value,
                                data.as_ref() as &[u8]
                            );
                        } else {
                            println!("{}", u32_value);
                        }
                    }
                };
            }
            _ => {
                return Err(From::from(format!(
                    "invalid byte size {} for variable {}",
                    size, name
                )));
            }
        }
    }

    Ok(true)
}

fn write_variable_value(
    picontrol: &mut picontrol::RevPiControl,
    name: &str,
    i32u_value: u32,
) -> Result<bool, Box<dyn std::error::Error>> {
    let spivariable = picontrol.get_variable_info(name)?;

    let mut spivalue: SPIValue = SPIValue {
        ..Default::default()
    };

    if spivariable.i16uLength == 1 {
        spivalue.i16uAddress = spivariable.i16uAddress;
        spivalue.i8uBit = spivariable.i8uBit;
        spivalue.i8uValue = i32u_value as u8;
        picontrol.set_bit_value(&mut spivalue)?;
    } else {
        /*
        match spivariable.i16uLength {
        8 => data = i32u_value as u8,
        16 => data = i32u_value as u16,
        32 => data = i32u_value as u32
        };
        */

        let bn = picontrol::num_to_bytes(i32u_value as u64, 32)?;
        println!("binary value: {:x?}", bn);

        picontrol.write(spivariable.i16uAddress as u64, &bn)?;
    }

    println!(
        "written value {} dec (={:x?} hex) to offset {}.\n",
        i32u_value, i32u_value, spivariable.i16uAddress
    );

    Ok(true)
}

fn show_device_list(as_dev_list: Vec<SDeviceInfo>) {
    let devcount = as_dev_list.len();

    println!("Found {} devices:", devcount);
    for &dev in &as_dev_list {
        // println!("Found {} devices:", dev.i16uModuleType);
        let mn = get_module_name(dev.i16uModuleType as u32);

        // Show device number, address and module type
        println!(
            "Address: {} module type: {} ({:x}) {} V{}.{}\n",
            dev.i8uAddress,
            dev.i16uModuleType,
            dev.i16uModuleType,
            mn,
            dev.i16uSW_Major,
            dev.i16uSW_Minor
        );

        if dev.i8uActive > 0 {
            println!("Module is present");
        } else if is_module_connected(dev.i16uModuleType as u32) {
            println!("Module is NOT present, data is NOT available!!!");
        } else {
            println!("Module is present, but NOT CONFIGURED!!!");
        }

        // Show offset and length of input section in process image
        println!(
            "    input offset: {} length: {}",
            dev.i16uInputOffset, dev.i16uInputLength
        );

        // Show offset and length of output section in process image
        println!(
            "    output offset: {} length: {}",
            dev.i16uOutputOffset, dev.i16uOutputLength
        );
        println!("\n")
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn create_clap_app() {
        super::create_clap_app().debug_assert();
    }
}
