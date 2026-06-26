# bore

[![Build status](https://img.shields.io/github/actions/workflow/status/fishandsheep/bore/ci.yml)](https://github.com/fishandsheep/bore/actions)
[![Crates.io](https://img.shields.io/crates/v/bore-cli.svg)](https://crates.io/crates/bore-cli)

[English](README.en.md)

`bore` 是一个用异步 Rust 编写的轻量 TCP 隧道工具。它可以把本地 TCP 端口暴露到远程服务器，适合绕过 NAT 或防火墙导致的入站连接限制。

## 最近更新

- 新增内置 Web 管理台：`bore --web` / `bore -w` 可在浏览器里管理 `local` 隧道。
- Web 管理台现在对隧道卡片做增量刷新；展开日志时只轮询日志，不再整块重绘 Tunnels 区域。
- 新增 npm / npx 分发：`@qinshower/bore` 会通过可选依赖安装当前平台的预编译二进制。
- 发布脚本支持从本地 package 路径准备并发布 npm 平台包。
- e2e 测试会等待控制端口释放，并在每个用例结束后停止测试服务器。
- `--bind-tunnels` 默认跟随 `--bind-addr`，两个参数都使用 IP 地址类型校验。
- TCP 转发改用 `copy_bidirectional`，并处理半关闭连接。

## 安装

### npx / bunx

```sh
npx @qinshower/bore local 8000 --to bore.pub
bunx @qinshower/bore local 8000 --to bore.pub
```

指定版本：

```sh
npx @qinshower/bore@0.6.1 --help
```

### Cargo

```sh
cargo install bore-cli
```

### 预编译二进制

从 [Releases](https://github.com/fishandsheep/bore/releases) 下载对应平台的压缩包，解压后把 `bore` 可执行文件放到 `PATH` 中。

## 快速使用

启动服务端：

```sh
bore server
```

把本地 `8000` 端口暴露到远程服务器：

```sh
bore local 8000 --to bore.pub
```

指定远程端口：

```sh
bore local 8000 --to bore.pub --port 9000
```

暴露非 `localhost` 的本地地址：

```sh
bore local 8080 --local-host 192.168.1.10 --to bore.pub
```

## Web 管理台

启动本地 Web 管理台：

```sh
bore --web
bore -w
```

默认监听地址：

```text
127.0.0.1:7836
```

指定监听地址：

```sh
bore --web --web-addr 127.0.0.1:9000
```

一期支持：

- 创建 `local` 隧道配置
- 启动 / 停止 `local` 隧道
- 查看隧道状态
- 查看最近日志
- 删除已停止或失败的隧道配置

当前交互细节：

- `Tunnels` 列表按卡片增量更新，运行中轮询不会整块闪动
- 仅在隧道运行中同步状态；如果只是展开日志，则只刷新日志输出
- 删除操作使用统一确认对话框
- 单条隧道动作会进入 busy 状态，避免重复点击和并发提交

注意：当前版本没有登录和认证。如果显式绑定到非 loopback 地址，启动时会打印安全警告。

## 自托管

在自己的机器上运行服务端：

```sh
bore server --bind-addr 0.0.0.0
```

客户端连接这台服务器：

```sh
bore local 8000 --to <SERVER_ADDRESS>
```

控制端口固定为 `7835`。隧道端口范围由 `--min-port` 和 `--max-port` 控制，默认是 `1024..=65535`。如果需要让控制连接和隧道监听在不同网卡上，可以设置 `--bind-addr` 和 `--bind-tunnels`。

## 认证

自托管服务端可以使用共享密钥限制访问：

```sh
# 服务端
bore server --secret my_secret_string

# 客户端
bore local 8000 --to <SERVER_ADDRESS> --secret my_secret_string
```

也可以通过 `BORE_SECRET` 环境变量传入密钥。密钥只保护握手过程；`bore` 本身不会加密隧道里的业务流量。

## 开发

```sh
cargo build
cargo test
cargo fmt --check
cargo clippy --all-targets --all-features
npm run npm:check
npm run npm:pack:dry-run
```

## 协议概要

服务端使用 `7835` 作为控制端口。客户端先发送 Hello 请求要暴露的远程端口；服务端接受外部 TCP 连接后生成 UUID，并通知客户端建立对应的 Accept 连接。服务端随后把两条 TCP 流互相转发。未被客户端接受的连接会在短时间后丢弃，避免资源泄露。

## 许可证

MIT。本仓库基于 Eric Zhang 创建的原始 `bore` 项目维护。
