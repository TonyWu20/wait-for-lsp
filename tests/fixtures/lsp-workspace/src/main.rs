fn greet(name: &str) {
    println!("Hello, {}!", name);
}

fn main() {
    let x: i32 = "not a number";
    greet(42);
    println!("{}", x);
}
