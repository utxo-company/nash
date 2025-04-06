module Nash.Parse.Module where

import Data.ByteString qualified as BS
import Data.Utf8 qualified

import Data.Name qualified as Name
import Nash.Ast.Source qualified as Src
import Nash.Package qualified as Pkg
import Nash.Parse.Declaration qualified as Decl
import Nash.Parse.Primitives hiding (State, fromByteString)
import Nash.Parse.Primitives qualified as P
import Nash.Reporting.Annotation qualified as A
import Nash.Reporting.Error.Syntax qualified as E

fromByteString :: ProjectType -> BS.ByteString -> Either E.Error Src.Module
fromByteString projectType source =
    case P.fromByteString (chompModule projectType) E.ModuleBadEnd source of
        Right modul -> checkModule projectType modul
        Left err -> Left (E.ParseError err)

-- PROJECT TYPE

data ProjectType
    = Package Pkg.Name
    | Application

-- MODULE

data Module
    = Module
    { _header :: Maybe Header
    , _imports :: [Src.Import]
    , _infixes :: [A.Located Src.Infix]
    , _decls :: [Decl.Decl]
    }

chompModule :: ProjectType -> Parser E.Module Module
chompModule projectType =
    do
        header <- chompHeader
        imports <- chompImports (if isCore projectType then [] else Imports.defaults)
        infixes <- chompInfixes []
        decls <- specialize E.Declarations $ chompDecls []
        return (Module header imports infixes decls)

-- HEADER

data Header
    = Header
        (A.Located Name.Name)
        Effects
        (A.Located Src.Exposing)
        (Either A.Region Src.Comment)

data Effects
    = NoEffects A.Region
    | Ports A.Region
    | Manager A.Region Src.Manager
