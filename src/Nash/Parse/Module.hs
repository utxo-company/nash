module Nash.Parse.Module where

import Data.ByteString qualified as BS
import Nash.Ast.Source qualified as Src
import Nash.Package qualified as Pkg
import Nash.Reporting.Error.Syntax qualified as E

fromByteString :: ProjectType -> BS.ByteString -> Either E.Error Src.Module
fromByteString _ _ = error "TODO: Implement fromByteString"

-- PROJECT TYPE

data ProjectType
    = Package Pkg.Name
    | Application
