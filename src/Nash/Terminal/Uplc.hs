module Nash.Terminal.Uplc
  ( module Nash.Terminal.Uplc.Fmt,
    module Nash.Terminal.Uplc.Eval,
    module Nash.Terminal.Uplc.Encode,
    module Nash.Terminal.Uplc.Decode,
    module Nash.Terminal.Uplc.Shrink,
    UplcCmd (..),
  )
where

import Nash.Terminal.Uplc.Decode
import Nash.Terminal.Uplc.Encode
import Nash.Terminal.Uplc.Eval
import Nash.Terminal.Uplc.Fmt
import Nash.Terminal.Uplc.Shrink

data UplcCmd
  = UplcFmt UplcFmtArgs
  | UplcEval UplcEvalArgs
  | UplcEncode UplcEncodeArgs
  | UplcDecode UplcDecodeArgs
  | UplcShrink UplcShrinkArgs
  deriving (Show)
