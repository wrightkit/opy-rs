#!define shout(x) __script__("shout.js")

globalvar S

rule "shout":
    S = shout("hello")
