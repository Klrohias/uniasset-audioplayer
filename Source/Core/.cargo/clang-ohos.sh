#!/bin/sh
exec "$OHOS_SDK_NATIVE/llvm/bin/clang" \
    --target="$OHOS_CLANG_TARGET" \
    --sysroot="$OHOS_SDK_NATIVE/sysroot" \
    "$@"
