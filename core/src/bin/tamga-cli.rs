//! Outil en ligne de commande de TAMGA (sans interface graphique).
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(tamga_core::cli::run(&args));
}
