module Nash.Parse.Declaration where

import Nash.Ast.Source qualified as Src
import Nash.Reporting.Annotation qualified as A

-- DECLARATION

data Decl
    = Value (Maybe Src.Comment) (A.Located Src.Value)
    | Union (Maybe Src.Comment) (A.Located Src.Union)
    | Alias (Maybe Src.Comment) (A.Located Src.Alias)
    | Port (Maybe Src.Comment) Src.Port
