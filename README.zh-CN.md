# Tollgate

[English](README.md) | 中文

一个本地运行的 LLM 网关:挡在你的 OpenAI/Anthropic 兼容客户端 SDK 前面,原样转发请求到真实上游,同时把用量/延迟/成本/完整请求响应内容记录到本地 SQLite 文件里。

由同一个核心 crate 构建出两种形态:

- **`tollgate`** —— 独立的 CLI/server 二进制。不带子命令运行即启动网关;用 `routes`/`keys`/`real-keys`/`pricing` 子命令来脚本化编辑 `config.yaml`。
- **桌面 App**(`app/`)—— 一个 Tauri + Svelte 的图形界面,同进程运行同一套网关逻辑,附带仪表盘、请求浏览器、routes/keys/pricing 编辑器。

## 为什么需要它

客户端 SDK(OpenAI 的、Anthropic 的,或任何直接发 HTTP 的)只需要一个 base URL 和一个 API key。把它们指向 `tollgate` 而不是真实的服务商,你就能在自己的机器上白嫖到:

- 逐请求日志:模型、token 数、成本、延迟、完整的请求/响应 body 和 headers,流式响应(SSE)按字节原样落库。
- 基于自己配置的美元/百万token定价表算成本。
- 一个 route 下可以挂多个上游("真实")key,每个 route 又能派发多个虚拟 key,分发受限凭证时永远不用暴露真实key。
- 通过 `x-session-id` 请求头做会话分组,方便在界面上把多轮对话串起来看。

## 路由是怎么匹配的

每个进来的请求,纯粹按它带的 API key(`Authorization: Bearer ...` 或 `x-api-key`)匹配到某个 route —— 不看路径、不看 host。route 的 `keys` 列表把虚拟key映射到该 route 的某个 `real_keys`;网关在转发前把虚拟key换成真实key,请求的其它部分(路径、query、body、其它 header)原样穿透。

## 快速开始(CLI)

```sh
cargo build --release

# 注册一个 route、它的上游凭证,以及一个对应的虚拟key
./target/release/tollgate routes add --name openai --format openai --upstream https://api.openai.com
./target/release/tollgate real-keys add --route openai --label main --value sk-...
./target/release/tollgate keys add --route openai --real-key main --label my-app
# -> 会打印生成的虚拟key;把这个给你的客户端 SDK 用,而不是真实key

./target/release/tollgate pricing add --model gpt-4o --input-per-mtok 2.5 --output-per-mtok 10

./target/release/tollgate   # 启动网关,默认监听 :8787
```

把客户端指向 `http://localhost:8787`,用 `keys add` 生成的虚拟key,用法和真实服务商的 base URL 完全一样。

配置文件默认存放在 OS 标准的 per-app 配置目录下(比如 macOS 上的 `~/Library/Application Support/com.tollgate.app/config.yaml`),这样 CLI 和桌面 App 开箱即用管理的是同一份文件。可以用 `--config <path>` 覆盖。

### CLI 命令参考

```
tollgate routes add --name <name> --format openai|anthropic --upstream <url>
tollgate routes list
tollgate routes remove --name <name>

tollgate real-keys add --route <route> --label <label> --value <upstream-api-key>
tollgate real-keys list [--route <route>] [--show-full]
tollgate real-keys remove --route <route> --label <label>

tollgate keys add --route <route> --real-key <real-key-label> [--value <v>] [--label <l>]
  (省略 --value 会自动生成一个虚拟key)
tollgate keys list [--route <route>] [--show-full]
tollgate keys remove --route <route> (--value <v> | --label <l>)

tollgate pricing add --model <model> --input-per-mtok <f> --output-per-mtok <f> [--cached-input-per-mtok <f>] [--cache-write-per-mtok <f>]
tollgate pricing list
tollgate pricing remove --model <model>
```

每条命令写入前都会校验,一次写坏的操作不会破坏配置文件。一个正在运行的独立 server 进程会在每次改动后通过 SIGHUP 自动热重载。

## 桌面 App

```sh
cd app
npm install
npm run tauri dev    # 开发模式
npm run tauri build   # 打包发布
```

App 是同进程运行网关的(不需要另外管理一个 server 进程),而且**不会**监听配置文件的外部改动 —— 手动改了 `config.yaml`(或通过 CLI 改)之后,需要重启 App 才生效。

## 配置文件参考

```yaml
listen: ":8787"          # 默认值
db_path: "./tollgate.db" # 默认值
max_body_bytes: 26214400 # 默认 25 MiB;超出的请求 body 会被拒绝(413)
shutdown_timeout: "30s"  # 可选;优雅关闭时等待存量请求处理完的最长时间

routes:
  - name: openai
    format: openai        # "openai" 或 "anthropic"
    upstream: https://api.openai.com
    real_keys:
      - label: main
        value: sk-...
    keys:
      - value: vk-...      # 和客户端 Authorization/x-api-key 逐字匹配
        label: my-app
        real_key: main

pricing:
  gpt-4o:
    input_per_mtok: 2.5
    output_per_mtok: 10.0
    cached_input_per_mtok: 1.25   # 可选,不设置则回退用 input_per_mtok
    cache_write_per_mtok: 3.75    # 可选,不设置则回退用 input_per_mtok
```

## 发布流程

推一个 `v*` tag 会触发 `.github/workflows/release.yml`,在 macOS(Apple Silicon)和 Windows 上分别构建桌面 App,并把两份产物发布成一个 GitHub Release。

## 项目结构

```
src/            核心网关/存储/配置/CLI 库 + `tollgate` 二进制
app/            Tauri + Svelte 桌面 App(同进程运行同一套核心逻辑)
app/src-tauri/  Tauri 宿主(Rust)
app/src/        Svelte 前端
```
