#!rulePrefixTemplate f"[{$prefix}] {$rule}" if $prefix and $rule and not $isDelimiter else $rule
#!optimizeForSize

globalvar a
globalvar __b__
globalvar c 4
globalvar __d
globalvar e
globalvar __f___
playervar pa
playervar __pb__
playervar pc 3
playervar pd

rule "Clear trigger admin﻿":
    @Event eachPlayer
    a = [x for x in e if x != 1]
    a = [x for x in e]
    e = ""
    a = vect(1, 0, 1)
    a = vect(255, 0, 0)
    a = vect(0, 0, -1)
    eventPlayer.pa = 1
    eventPlayer.__pb__ = 1
    eventPlayer.pc = 1
    eventPlayer.pd = 1
    __b__ = 1
    __d = 1
    __f___ = 1
    c = 1
    a = vect(1, 0, 0)
    a = vect(-1, 0, 0)
    a = vect(0, -1, 0)
    a = vect(0, 0, 1)
    a = vect(0, 1, 0)
