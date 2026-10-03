// swift-tools-version: 6.2
import PackageDescription
let package = Package(
    name: "HexagonComputerUse",
    platforms: [.macOS(.v15)],
    products: [.library(name: "HexagonComputerUse", type: .dynamic, targets: ["HexagonComputerUse"])],
    dependencies: [.package(url: "https://github.com/openclaw/Peekaboo.git", revision: "f3c6b1b32528290a959244d1ac12d54af3d42928")],
    targets: [.target(name: "HexagonComputerUse", dependencies: [.product(name: "PeekabooAutomationKit", package: "Peekaboo")]),
              .testTarget(name: "HexagonComputerUseTests", dependencies: ["HexagonComputerUse"])]
)
