module Nash.Parse.Module where

import Data.ByteString qualified as BS
import Data.Utf8 qualified
import Nash.Ast.Source qualified as Src
import Nash.Package qualified as Pkg
import Nash.Reporting.Error.Syntax qualified as E

fromByteString :: ProjectType -> BS.ByteString -> Either E.Error Src.Module
fromByteString _ _ = Left $ E.ModuleNameUnspecified (Data.Utf8.fromChars "thing")

-- PROJECT TYPE

data ProjectType
    = Package Pkg.Name
    | Application
