# Run binary
dev *args:
    @go run ./cmd/nashc {{args}}

# Clean up modules
tidy:
    @echo "run: tidy"

    @go mod tidy

# Formats the code
fmt:
    @echo "run: formatter"

    @go fmt ./...

    @golines -w --ignore-generated --chain-split-dots --max-len=80 --reformat-tags .

# Run tests
test *args:
    @echo "run: tests"

    @go mod tidy

    @go test -v -race {{args}} ./...

## Remove test cache
clean:
	@go clean -testcache

	@rm -rf cmd/_out

build *name:
    #!/usr/bin/env sh
    mkdir -p cmd/_out

    if [ -n "{{name}}" ]; then
        echo "build: {{name}}"

        go build -o cmd/_out/{{name}} ./cmd/{{name}}
    else
        echo "build: all"

        go build -o cmd/_out ./cmd/...
    fi

install:
    go install github.com/segmentio/golines@latest
    go install golang.org/x/tools/cmd/goimports@latest
