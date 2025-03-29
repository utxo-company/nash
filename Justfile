run *args:
    cabal run nash -- {{args}}

# MacOS specific system dependencies
setup:
    #!/usr/bin/env bash

    brew install libsodium
    brew install pkgconf
    brew install secp256k1

    echo "Installing blst"

    git clone https://github.com/supranational/blst.git

    cd blst

    bash build.sh

    sudo mkdir /usr/local/lib
    sudo mkdir /usr/local/lib/pkgconfig
    sudo mkdir /usr/local/include

    sudo cp libblst.a /usr/local/lib
    sudo cp bindings/blst.h bindings/blst_aux.h /usr/local/include

    cat > /usr/local/lib/pkgconfig/libblst.pc << 'EOF'
    prefix=/usr/local
    exec_prefix=${prefix}
    libdir=${exec_prefix}/lib
    includedir=${prefix}/include

    Name: blst
    Description: Multilingual BLS12-381 signature library
    Version: 0.3.10
    Libs: -L${libdir} -lblst
    Cflags: -I${includedir}
    EOF

    rm -rf blst

    pkg-config --modversion libblst
