# @qinshower/bore

通过 npm 兼容的包运行器执行 Rust 版 `bore` 二进制。

```sh
npx @qinshower/bore local 8000 --to bore.pub
bunx @qinshower/bore local 8000 --to bore.pub
```

可通过 npm semver 指定版本。

```sh
npx @qinshower/bore@0.6.3 --help
```

`npx` 可能会先消费 `-w` 这类前导参数。启动 Web 管理台请使用：

```sh
npx @qinshower/bore web
npx @qinshower/bore -- -w
```

远端公开 Web 管理台：

```sh
npx @qinshower/bore web --remote --to your-server.com --port 7836 --secret xxx
```

`home` 组合模式：

```sh
npx @qinshower/bore home --to your-server.com --secret xxx
```

注意：Web 管理台当前没有浏览器登录和认证。若使用 `web --remote` 或 `home`，任何能访问远端 `server:<web-port>` 的人都能控制本机 loopback tunnels。

更多用法见主仓库文档：https://github.com/fishandsheep/bore
