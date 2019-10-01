extern crate clap;

use clap::{App, Arg};

fn main() {
    // Once all App settings (including all arguments) have been set, you call get_matches() which
    // parses the string provided by the user, and returns all the valid matches to the ones you
    // specified.
    //
    // You can then query the matches struct to get information about how the user ran the program
    // at startup.
    //
    // For this example, let's assume you created an App which accepts three arguments (plus two
    // generated by clap), a flag to display debugging information triggered with "-d" or
    // "--debug" as well as an option argument which specifies a custom configuration file to use
    // triggered with "-c file" or "--config file" or "--config=file" and finally a positional
    // argument which is the input file we want to work with, this will be the only required
    // argument.
    let matches = App::new("MyApp")
        .about("Parses an input file to do awesome things")
        .version("1.0")
        .author("Kevin K. <kbknapp@gmail.com>")
        .arg(
            Arg::with_name("debug")
                .help("turn on debugging information")
                .short('d')
                .long("debug"),
        )
        .arg(
            Arg::with_name("config")
                .help("sets the config file to use")
                .short('c')
                .long("config"),
        )
        .arg(
            Arg::with_name("input")
                .help("the input file to use")
                .index(1)
                .required(true),
        )
        .get_matches();

    // We can find out whether or not debugging was turned on
    if matches.is_present("debug") {
        println!("Debugging is turned on");
    }

    // If we wanted to some custom initialization based off some configuration file provided
    // by the user, we could get the file (A string of the file)
    if let Some(ref file) = matches.value_of("config") {
        println!("Using config file: {}", file);
    }

    // Because "input" is required we can safely call unwrap() because had the user NOT
    // specified a value, clap would have explained the error the user, and exited.
    println!(
        "Doing real work with file: {}",
        matches.value_of("input").unwrap()
    );

    // Continued program logic goes here...
}
