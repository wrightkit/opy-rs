#!include "../constants/"
#!include "../env/vars.opy"
#!defineMember BOOTSTRAP_VALUE 1

settings {
    "main": {
        "description": FULL_DESCRIPTION
    },
    "gamemodes": {
        "general": {
            "respawnTime%": ENTRY_VALUE
        }
    }
}

#!rulePrefixTemplate f"[{$prefix}] {$rule}" if $prefix and $rule and not $isDelimiter else $rule

rule "bootstrap":
    @Event global
    pass
