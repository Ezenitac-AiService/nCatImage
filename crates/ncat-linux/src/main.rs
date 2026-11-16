#![forbid(unsafe_code)]

mod network;

fn main() -> gtk::glib::ExitCode {
    ncat_gtk::run(|file, parent| Box::pin(network::resolve(file, parent)))
}
