#!rulePrefixTemplate
globalvar reserved 0
globalvar first
globalvar explicit 2
globalvar next
subroutine helper

def helper():
    @Name "renamed helper"
    @SuppressWarnings unusedVariable
    first = 1

rule "joined":
    @Event playerJoined
    @Team 1
    @Slot 2
    @SuppressWarnings unusedVariable
    helper()
