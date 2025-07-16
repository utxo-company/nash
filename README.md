# nash

## dev

- install [blst](https://github.com/supranational/blst)
- install [secp256k1](https://github.com/bitcoin-core/secp256k1)
  - brew install secp256k1

## Docs

### Imports

```
import { someFunc } from @microproofs/mpf
import tree from @microproofs/mpf/tree

import (
    { someFunc } from @microproofs/mpf
    thing from @microproofs/mpf/tree
)

// Without from
import @microproofs/mpf.{someFunc}
import @microproofs/mpf/tree

import (
    @microproofs/mpf.{someFunc}
    @microproofs/mpf/tree as thing
)
```

modules defined with `~` at the start of the name behave like `index.ts` in typescript modules

### Defining types

```
interface Thing {
  cmp(a, a) Ordering
}

type Wow = {
  thing: Int,
  next: Int,
  fiilll: Int,
}

type Something {
  Thing Wow
  Who (Wow)
  Me { thing: Int, next: Int }
}

when something is {
  Thing wow -> {
    wow.thing
  }
  Anal wow -> {
    wow.0.thing
  }
}
```
