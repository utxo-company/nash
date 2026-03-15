test:
    cargo nextest run

    cargo bench

download-plutus-tests:
    set -euo pipefail

    rm -rf crates/nash-plutus/tests/conformance

    curl -L -s https://github.com/IntersectMBO/plutus/archive/master.tar.gz | tar xz -C /tmp

    mkdir -p crates/nash-plutus/tests/conformance

    mv /tmp/plutus-master/plutus-conformance/test-cases/uplc/evaluation/* crates/nash-plutus/tests/conformance/

    rm -rf /tmp/plutus-master

    @echo "Download complete. Test cases are now in crates/nash-plutus/tests/conformance/"

generate-conformance-tests:
    cargo run -p generate-conformance-tests
