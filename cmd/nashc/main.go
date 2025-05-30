package main

import (
	"fmt"
	"os"

	"github.com/nash-script/compiler/pkg/syn"
)

func main() {
	fmt.Println(syn.Thing)
	fmt.Println(os.Args[1:])
}
