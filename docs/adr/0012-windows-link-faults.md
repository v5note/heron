# ADR-0012: 四个只在真实 Windows 上暴露的链接层缺陷

- 状态：已采纳（2026-11-19）
- 相关：实现文档 §4.3 / §10.2、风险 R3

## 背景

Windows 链接安全测试（L1–L6）此前只在**静态编译**层面验证过，从未在真实机器上跑。
首次推送后 CI 立即失败，连续暴露四个缺陷。它们共同的特征是：**在 macOS 上测试永远通过**。

## 缺陷与修复

### 1. 读取 reparse point 用了 GENERIC_WRITE

CreateFileW 以 GENERIC_WRITE 打开链接，在非提权环境（CI runner）下对非自己创建的链接
返回 ERROR_ACCESS_DENIED，且原代码**静默返回 (None, None)**，于是每个 probe 都报告
“目标不存在”。

→ 改为只读访问（访问权限传 0）；读取失败时**不再吞掉错误**，而是返回真实 Win32 错误码。

### 2. 符号链接的 PathBuffer 偏移算错

REPARSE_DATA_BUFFER 中，junction 的 PathBuffer 在偏移 16，而 symlink 前面多一个
ULONG Flags，所以从 **20** 开始。原代码对两者都用 16，读出的名字整体错位四个字节，
产生形如 g2\??\C:\... 的垃圾路径。

→ 按 tag 区分起点：偏移 16，若为 symlink 再加 4。

### 3. NT 前缀的转义多了一层

剥除 \\?\ 或 \??\ 前缀的字面量写成了四个反斜杠，永远匹配不上；
路径保留了前缀，被当作相对路径解析。

→ 定义为命名常量 VERBATIM_PREFIX / NT_PREFIX，并用小程序实测剥离结果。

### 4. 悬空链接被当成 SymbolicFile 且无法删除

目标消失后 target.is_dir() 为假，链接被判成文件类型，删除走 remove_file，
对目录符号链接报 ERROR_ACCESS_DENIED。即使走对了分支，
FSCTL_DELETE_REPARSE_POINT 也因为**输入缓冲区为空**而失败。

→ 用 FileTypeExt::is_symlink_dir 直接读链接本身分类；删除时传入 8 字节头部
（tag + 零长度负载）。

## 方法论

每一步都不是猜的：

1. 先让失败**可见**（把静默的 (None, None) 改成返回错误码）；
2. 再加临时诊断，把**原始字节**打进 CI 日志（0c 00 00 a0 | 76 00 | 7e 00 ...）；
3. 用字节反推结构体布局，确认偏移；
4. 修复后立刻删除诊断代码。

## 后果

- Windows 上 229 项测试全绿，其中包括此前从未真正跑过的 L1–L6；
- 链接层的错误不再被静默吞掉；
- 同时修掉两处**测试自身**的平台假设（ends_with 字面量路径、assert_eq 硬编码 /home/u），
  它们只会在 Windows 上失败。
