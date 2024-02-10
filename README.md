<!-- PROJECT LOGO -->
<br />
<h3 align="center">AtomBlocks</h3>

  <p align="center">
    async, absolutely lightweight and dead simple bar for dwm and similar window managers
    <br />
    <br />
    <a href="https://github.com/milchinskiy/atomblocks/issues">Report Bug</a>
    ·
    <a href="https://github.com/milchinskiy/atomblocks/issues">Request Feature</a>
  </p>
</div>



<!-- ABOUT THE PROJECT -->
## About The Project

another bar implementation for the DWM window manager and similar ones, with asynchronous and independent blocks update.


<!-- GETTING STARTED -->
## Getting Started

To get a local copy up and running follow these simple example steps.

### Prerequisites

Install Rust and Cargo. The easiest way to get Cargo is to install the current stable release of Rust by using rustup. Installing Rust using rustup will also install cargo.
* install rustup:
  ```sh
  curl https://sh.rustup.rs -sSf | sh
  ```

* install stable rust and cargo:
  ```sh
  rustup install stable
  ```

### Build from sources

1. Clone the repo
   ```sh
   git clone https://github.com/milchinskiy/atomblocks.git && cd ./atomblocks
   ```
2. Build release
   ```sh
   cargo build --release
   ```

### Install from crates.io
```sh
cargo install atomblocks
```


### Install from AUR (Arch Linux)

using `yay`, `paru` or any other AUR helper you prefer, yay for example:

```sh
yay -S atomblocks
```

### Install via Nix Flakes

_work in progress..._


<p align="right">(<a href="#readme-top">back to top</a>)</p>



<!-- USAGE EXAMPLES -->
## Usage

An incredibly simple and straightforward configuration file can be found in the code repository [sample/config.toml](sample/config.toml)
There are a few places where configuration could live

* `$XDG_CONFIG_HOME`/atomblocks/config.toml
* `$HOME`/.config/atomblocks/config.toml
* /etc/atomblocks/config.toml
* you can directly set the file via args


### Run

```sh
atomblocks run
```

### Manually hit the block to update

```sh
atomblocks hit <ID>
# where <ID> is a sequential block index in config file
```

### Run with custom config

```sh
atomblocks run --config ./my-custom-config.toml
```




<p align="right">(<a href="#readme-top">back to top</a>)</p>



### TODO

- [ ] Delivery methods
    - [ ] Nix Flakes
    - [x] AUR
    - [x] crates.io




<!-- CONTRIBUTING -->
## Contributing

Contributions are what make the open source community such an amazing place to learn, inspire, and create. Any contributions you make are **greatly appreciated**.

If you have a suggestion that would make this better, please fork the repo and create a pull request. You can also simply open an issue with the tag "enhancement".
Don't forget to give the project a star! Thanks again!

1. Fork the Project
2. Create your Feature Branch (`git checkout -b feature/AmazingFeature`)
3. Commit your Changes (`git commit -m 'Add some AmazingFeature'`)
4. Push to the Branch (`git push origin feature/AmazingFeature`)
5. Open a Pull Request



<!-- LICENSE -->
## License

Distributed under the MIT License. See `LICENSE` file for more information.



<!-- MARKDOWN LINKS & IMAGES -->
<!-- https://www.markdownguide.org/basic-syntax/#reference-style-links -->
[contributors-shield]: https://img.shields.io/github/contributors/milchinskiy/atomblocks.svg?style=for-the-badge
[contributors-url]: https://github.com/milchinskiy/atomblocks/graphs/contributors
[forks-shield]: https://img.shields.io/github/forks/milchinskiy/atomblocks.svg?style=for-the-badge
[forks-url]: https://github.com/milchinskiy/atomblocks/network/members
[stars-shield]: https://img.shields.io/github/stars/milchinskiy/atomblocks.svg?style=for-the-badge
[stars-url]: https://github.com/milchinskiy/atomblocks/stargazers
[issues-shield]: https://img.shields.io/github/issues/milchinskiy/atomblocks.svg?style=for-the-badge
[issues-url]: https://github.com/milchinskiy/atomblocks/issues
