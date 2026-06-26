# @qinshower/bore

通过 npm 兼容的包运行器执行 Rust 版 `bore` 二进制。

```sh
npx @qinshower/bore local 8000 --to bore.pub
bunx @qinshower/bore local 8000 --to bore.pub
```

可通过 npm semver 指定版本。

```sh
npx @qinshower/bore@0.6.2 --help
```

`npx` 可能会先消费 `-w` 这类前导参数。启动 Web 管理台请使用：

```sh
npx @qinshower/bore web
npx @qinshower/bore -- -w
```

更多用法见主仓库文档：https://github.com/fishandsheep/bore
