#!allowMacroRedeclaration
#!translations en fr
#!rulePrefix "Effects"
#!rulePrefixTemplate
#!disableOptimizations
#!enableOptimizations
#!optimizeForSize
#!optimizeForSizeAggressive
#!disableOptimizeForSize
#!optimizeStrict
#!disableOptimizeStrict
#!replace0ByCapturePercentage
#!replace1ByMatchRound
#!replaceTeam1ByControlScoringTeam
#!replaceEmptyStringByEmptyArray
#!define VALUE 1
#!define VALUE 2
globalvar value

subroutine helper

def helper():
    @Name "renamed helper"
    @SuppressWarnings unusedVariable
    value = VALUE

rule "directive annotations":
    @Event eachPlayer
    @Team 1
    @Hero dmon
    @Disabled
    @Delimiter
    @NewPage "Page"
    @SuppressWarnings unusedVariable
    value = VALUE
