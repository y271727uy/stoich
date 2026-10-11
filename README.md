stoich 是 stoichiometry（化学计量）的缩写，来自希腊语 stoicheion（组成）和 metron（度量）。它指反应物和生成物之间的数量比

这并不是AE2附属通用库！！当前仅为实验性的算法库，仅用于验证超大量级运算的实现。

## 调用

物品编号由调用方完成。求解器编进这个 jar，由 JVM 进程内加载，ECO、DataEnergistics、thunderbolt、ae2lt 依赖这一份，不要各自再嵌一份。

```java
StoichGraph graph = StoichGraph.builder(recipes, items)
    .column(0, new int[]{0, 1}, new long[]{9, 1}, new long[]{10, 0})
    .stock(0, 9)
    .target(0, 1_000_000_009L)
    .importable(1)
    .build();

StoichResult result = Stoich.solve(graph);
```

第一次调用把对应平台的动态库释放到游戏目录的 `.stoich/`，之后复用这个文件。Windows 是 `stoich-<版本>.dll`，Linux 是 `libstoich-<版本>.so`，macOS 是 `libstoich-<版本>.dylib`。当前构建只产出 Windows x86_64。

## 布局

- `rust/stoich-kernel`：块代数、次数界、单配方增长、固定顺序、压缩搜索。
- `rust/stoich-worker`：动态库。Java 通过 JNI 传入一条 protobuf 请求，拿回一条 protobuf 响应。
- `src/main/java/y271727uy/stoich/api`：调用方唯一要导入的包，`Stoich`、`StoichGraph`、`StoichResult`。
- `src/main/java/y271727uy/stoich/internal`：动态库加载和帧编解码，不导出。

