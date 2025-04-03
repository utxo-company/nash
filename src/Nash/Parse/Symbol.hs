module Nash.Parse.Symbol where

data BadOperator
    = BadDot
    | BadPipe
    | BadArrow
    | BadEquals
    | BadHasType
    deriving (Show)
