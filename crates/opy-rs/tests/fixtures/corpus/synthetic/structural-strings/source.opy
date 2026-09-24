#!define P "{0}\r\n{1}\r\n{2}{3}"
#!define D "a \n{0}/{1}"
globalvar a
globalvar b
globalvar c
globalvar t

rule "strings":
    @Event global
    t = P.format(D.format(a, b), "x {0} {1}".format(c, 2.5), "{}".format(null), "y" if a else "")
    t = "{0}{0}{1}".format(a, b)
    t = "n {0} {1} {2}".format(3.14159, 0.001, 1234567.891)
    t = "abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz{0}tail-tail-tail-tail-tail-tail-tail".format(a)
