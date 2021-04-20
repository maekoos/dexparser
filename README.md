## WARNING
This repo contains a bodge trying to fix how tries are handled in the original dexparser crate. It is only in the bodge-state for now since I'm not sure how it's supposed to be implemented - and I need this functionality for the [aar](https://github.com/maekoos/aar) project.

# Dexparser

A Rust library for parsing Android's DEX file format with parser combinators.

## What is DEX?

The Dalvik Executable (DEX) format is a file format used by Android to encode compiled Dalvik bytecode. It is distributed as part of a packaged Android application package (APK) and executed by Android phones.

The best reference for the format is the [official document](https://source.android.com/devices/tech/dalvik/dex-format), which this library is based off.

## Usage
```
    let mut file = File::open(path).unwrap();
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes);

    match dexparser::parse(&bytes) {
        Ok(res) => { /* do something */ },
        Err(e) => { /* handle error */ }
    }
```

## License

This project is licensed under the MIT License - see the [LICENSE.md](LICENSE.md) file for details