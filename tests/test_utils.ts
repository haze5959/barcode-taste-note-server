import { IResponse } from "https://deno.land/x/superoak/mod.ts";

export function testCheck(res: IResponse) {
    console.log(res.body['result']);
    if (res.body['result']) {
        console.log(res.body);
        return true
    } else {
        console.log(res.body);
        return false
    }
}