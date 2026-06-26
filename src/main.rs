fn run() -> &'static str {
    "git-shadow (git-shad): placeholder — not yet implemented."
}

fn main() {
    println!("{}", run());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_message() {
        assert_eq!(
            run(),
            "git-shadow (git-shad): placeholder — not yet implemented."
        );
    }
}
