# CMake reads this file when it builds PROJ for iOS. `build-rust.sh` sets
# VALE_IOS_SDK to `iphoneos` or to `iphonesimulator`.
set(CMAKE_SYSTEM_NAME iOS)
set(CMAKE_SYSTEM_PROCESSOR arm64)
set(CMAKE_OSX_ARCHITECTURES arm64)
set(CMAKE_OSX_SYSROOT "$ENV{VALE_IOS_SDK}")

# The PROJ build runs `sqlite3` on the Mac. It links the SQLite of the iOS SDK.
set(CMAKE_FIND_ROOT_PATH_MODE_PROGRAM NEVER)
