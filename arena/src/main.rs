mod auth;
mod authz;
mod db;
mod driver;
mod keys;
mod manifest;
mod naming;

#[tokio::main]
async fn main() {
    println!("arena: see `arena --help` (CLI lands in Task 11)");
}
