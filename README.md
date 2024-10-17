# Clipshare

Reference <https://github.com/reu/clipshare>

Just enough for me

## How to use

```bash
clipshare --help
```

```text
Share clipboard between machines on your local network

Usage: clipshare [OPTIONS]

Options:
  -p, --port <PORT>  Server port
  -u, --url <URL>    Remote server url
      --no-clear     Don´t clear the clipboard on start
  -h, --help         Print help
  -V, --version      Print version
```

On one machine:

```bash
clipshare --port 11337
```

And then on another machine on the same network

```bash
clipshare --url ip:11337
```
