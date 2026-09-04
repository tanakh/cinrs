/* Found through `#pragma cinrs include_path "tests/include"` and included with
 * angle brackets, which never look in the including file's own directory. */
#ifndef MYHEADER_H
#define MYHEADER_H

#define MY_ANSWER 42

int my_answer(void);

#endif /* MYHEADER_H */
