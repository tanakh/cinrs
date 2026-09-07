/* A file small enough to be included inside a function body, where the
 * expansion's module and its glob re-export are block items like any other. */
#pragma cinrs safe inner_double

int inner_double(int n)
{
    return n * 2;
}
