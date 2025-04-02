module Nash.String where

import Data.Utf8 qualified as Utf8

-- STRINGS

type String =
    Utf8.Utf8 ELM_STRING

data ELM_STRING
