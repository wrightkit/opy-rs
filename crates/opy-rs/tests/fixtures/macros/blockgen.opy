#!define gen() __script__("blockgen.js")

globalvar A
globalvar B
globalvar C

rule "script block":
    @Event global
    gen()
