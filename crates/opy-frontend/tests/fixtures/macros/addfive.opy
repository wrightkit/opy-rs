#!define addFive(x) __script__("addfive.js")

globalvar A

rule "add five":
    A = addFive(3)
